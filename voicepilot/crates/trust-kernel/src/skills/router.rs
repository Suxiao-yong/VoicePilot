//! SkillRouter — V1.1 §5.1.
//!
//! W3b: 确定性关键词匹配。两种策略,按顺序尝试:
//!   1. 双向 substring 匹配 `intent_examples`(goal 包含 example OR example 包含 goal)
//!   2. substring 匹配精选 `keywords`(如 "整理" 同时出现在 goal 和 Skill)
//!
//! 首个匹配的 Skill 胜出。大小写不敏感。
//!
//! W7: keyword 优先,LLM fallback(置信度 >= 0.7 才用 LLM 结果)。
//! 路由顺序:
//!   1. intent_examples 命中 → score = 1.0 → 直接返回 Skill
//!   2. keywords 命中 → score = 0.5 → 直接返回 Skill
//!   3. score < 0.5 且 LLM 启用 → 调 LLM,confidence >= 0.7 返回 SkillWithSlots
//!   4. 否则 → Planner

#[cfg(feature = "llm")]
use std::sync::Arc;

#[cfg(feature = "llm")]
use crate::llm::client::LlmClient;
#[cfg(feature = "llm")]
use crate::llm::types::ExtractedSlot;
use crate::skills::manifest::SkillManifest;

#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(Box<SkillManifest>),
    /// W7 新增:LLM 提取了 Slot,经 UI 反馈给用户修改/Apply 后再执行
    #[cfg(feature = "llm")]
    SkillWithSlots(Box<SkillManifest>, Vec<ExtractedSlot>),
    /// W8 Plan 4 新增:LLM 拆解为多步 DAG(spec §2.8)。
    /// 调用方(CLI / Tauri)收到此变体时应弹 DAG 骨架审批 UI(Plan 5 实现)。
    /// 同步 `route()` 从不返回此变体(Dag 需要 LLM 调用,async only)。
    #[cfg(feature = "llm")]
    Dag(crate::skills::dag_types::DagPlan),
    Planner,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRouter {
    skills: Vec<SkillManifest>,
    /// W7: 可选 LLM 客户端(None = 无 LLM,纯关键词匹配)
    #[cfg(feature = "llm")]
    llm: Option<Arc<LlmClient>>,
}

impl SkillRouter {
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            #[cfg(feature = "llm")]
            llm: None,
        }
    }

    /// W7: 注入 LLM 客户端,启用 LLM fallback 路由
    #[cfg(feature = "llm")]
    pub fn with_llm(llm: Arc<LlmClient>) -> Self {
        Self {
            skills: Vec::new(),
            llm: Some(llm),
        }
    }

    /// W7 Plan 3: 若已存在同 id 的 Skill,覆盖(用户自定义 > built-in);
    /// 否则 push。这保证用户在 `%APPDATA%\voicepilot\skills\` 放入同 id
    /// 的 .md 文件后,能覆盖 built-in manifest。
    pub fn register(&mut self, manifest: SkillManifest) {
        if let Some(existing) = self.skills.iter_mut().find(|s| s.id == manifest.id) {
            *existing = manifest;
        } else {
            self.skills.push(manifest);
        }
    }

    /// W8 Plan 4: 返回已注册的 Skill manifest 列表(供 LLM decompose_to_dag
    /// 作为候选 Skill 传入)。spec §2.2 `decompose_to_dag(text, candidate_skills, user_slots)`。
    pub fn skills(&self) -> &[SkillManifest] {
        &self.skills
    }

    /// 同步路由(W6 行为,关键词匹配,不调 LLM)
    pub fn route(&self, user_goal: &str) -> RouteDecision {
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            if self.matches_intent_examples(&goal_lower, skill)
                || self.matches_keywords(&goal_lower, skill)
            {
                return RouteDecision::Skill(Box::new(skill.clone()));
            }
        }
        RouteDecision::Planner
    }

    /// W7: 异步路由(关键词优先,LLM fallback)
    #[cfg(feature = "llm")]
    pub async fn route_with_llm(&self, user_goal: &str) -> RouteDecision {
        // Step 1-2: keyword 匹配(与 route() 相同)
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            if self.matches_intent_examples(&goal_lower, skill)
                || self.matches_keywords(&goal_lower, skill)
            {
                return RouteDecision::Skill(Box::new(skill.clone()));
            }
        }

        // Step 3: LLM fallback
        if let Some(llm) = &self.llm {
            if llm.is_enabled() {
                match llm.classify_and_extract(user_goal, &self.skills).await {
                    Ok(resp) if resp.confidence >= 0.7 => {
                        if let Some(skill_id) = &resp.matched_skill_id {
                            if let Some(skill) = self.skills.iter().find(|s| &s.id == skill_id) {
                                if resp.slots.is_empty() {
                                    return RouteDecision::Skill(Box::new(skill.clone()));
                                }
                                return RouteDecision::SkillWithSlots(
                                    Box::new(skill.clone()),
                                    resp.slots,
                                );
                            }
                        }
                    }
                    _ => {} // LLM 失败或低置信度,回退到 Planner
                }
            }
        }

        // Step 4: Planner
        RouteDecision::Planner
    }

    fn matches_intent_examples(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for example in &skill.intent_examples {
            let example_lower = example.to_lowercase();
            if goal_lower.contains(&example_lower) || example_lower.contains(goal_lower) {
                return true;
            }
        }
        false
    }

    fn matches_keywords(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for keyword in &skill.keywords {
            let keyword_lower = keyword.to_lowercase();
            if !keyword_lower.is_empty() && goal_lower.contains(&keyword_lower) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::manifest::files_organize_manifest;

    #[test]
    fn route_keyword_match_returns_skill() {
        let mut router = SkillRouter::new();
        router.register(files_organize_manifest());
        let decision = router.route("整理下载目录");
        assert!(matches!(decision, RouteDecision::Skill(_)));
    }

    #[test]
    fn route_no_match_returns_planner() {
        let mut router = SkillRouter::new();
        router.register(files_organize_manifest());
        let decision = router.route("totally unrelated text");
        assert!(matches!(decision, RouteDecision::Planner));
    }

    #[test]
    fn register_same_id_overrides_built_in() {
        let mut router = SkillRouter::new();
        let mut built_in = files_organize_manifest();
        built_in.title = "Built-in".to_string();
        router.register(built_in);

        let mut user = files_organize_manifest();
        user.title = "User override".to_string();
        router.register(user); // same id, should override

        let decision = router.route("整理下载目录");
        match decision {
            RouteDecision::Skill(m) => assert_eq!(m.title, "User override"),
            _ => panic!("expected Skill decision"),
        }
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_with_llm_keyword_high_score_skips_llm() {
        // 用无效 URL 的 LlmClient — 若 LLM 被调用会失败,但 keyword 命中应跳过 LLM
        let llm = Arc::new(LlmClient::new(
            "https://invalid.example.com",
            "sk-test",
            "test",
        ));
        let mut router = SkillRouter::with_llm(llm);
        router.register(files_organize_manifest());
        // "整理下载目录" 命中 keyword "整理"
        let decision = router.route_with_llm("整理下载目录").await;
        assert!(matches!(decision, RouteDecision::Skill(_)));
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_with_llm_no_keyword_disabled_returns_planner() {
        let llm = Arc::new(LlmClient::disabled());
        let mut router = SkillRouter::with_llm(llm);
        router.register(files_organize_manifest());
        // "请讲解量子计算原理" 不命中任何 keyword(整理/归档/移动文件/下载目录),
        // LLM disabled → Planner
        let decision = router.route_with_llm("请讲解量子计算原理").await;
        assert!(matches!(decision, RouteDecision::Planner));
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_with_llm_no_keyword_invalid_llm_returns_planner() {
        // LLM 启用但 URL 无效 → LLM 调用失败 → 回退 Planner
        let llm = Arc::new(LlmClient::new(
            "https://invalid.example.com",
            "sk-test",
            "test",
        ));
        let mut router = SkillRouter::with_llm(llm);
        router.register(files_organize_manifest());
        // 同上,避开 keyword 命中
        let decision = router.route_with_llm("请讲解量子计算原理").await;
        assert!(matches!(decision, RouteDecision::Planner));
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn route_with_llm_high_confidence_returns_skill_with_slots() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": "{\"matched_skill_id\":\"files.organize\",\"confidence\":0.9,\"slots\":[{\"kind\":\"path\",\"raw\":\"C:\\\\Downloads\",\"high_risk\":true}],\"reasoning\":\"user wants to organize files\"}"
                        }
                    }]
                }
            }]
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
        let mut router = SkillRouter::with_llm(llm);
        router.register(files_organize_manifest());
        // "请讲解量子计算原理" 不命中任何 keyword,但 LLM mock 返回 files.organize
        // + confidence 0.9 + 1 slot → SkillWithSlots
        let decision = router.route_with_llm("请讲解量子计算原理").await;
        match decision {
            RouteDecision::SkillWithSlots(manifest, slots) => {
                assert_eq!(manifest.id, "files.organize");
                assert_eq!(slots.len(), 1);
                assert_eq!(slots[0].kind, "path");
            }
            other => panic!("expected SkillWithSlots, got {:?}", other),
        }
    }

    // ===== W8 Plan 4 Task 1: RouteDecision::Dag 变体 + skills() accessor =====

    #[cfg(feature = "llm")]
    #[test]
    fn route_decision_dag_variant_constructs_and_matches() {
        use crate::skills::dag_types::{DagNode, DagPlan};
        use crate::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
        use crate::policy::types::ELevel;
        use std::collections::HashMap;

        let node = DagNode {
            node_id: "n1".into(),
            skill_id: "note.capture".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("notepad".into()),
            },
            risk_ceiling: ELevel::E1,
        };
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![node],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        let decision = RouteDecision::Dag(plan);
        match decision {
            RouteDecision::Dag(p) => {
                assert_eq!(p.plan_id, "p1");
                assert_eq!(p.nodes.len(), 1);
                assert_eq!(p.nodes[0].skill_id, "note.capture");
            }
            _ => panic!("expected Dag variant"),
        }
    }

    #[cfg(feature = "llm")]
    #[test]
    fn skill_router_skills_accessor_returns_registered() {
        let mut router = SkillRouter::new();
        router.register(files_organize_manifest());
        let skills = router.skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "files.organize");
    }
}
