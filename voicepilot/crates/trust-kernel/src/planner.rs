//! Wave 1 Task 1.2: PlannerPipeline — 纯规划层(side-effect-free)。
//!
//! 文本 / 语音 / CLI 共用同一个无执行副作用的规划入口:
//! `plan()` 只做路由决策并返回 `(PlanResult, PlannerTrace)`,不创建 task、
//! 不写 audit、不写 taint、不执行工具。
//!
//! Trace 持久化由调用方(任务运行层,如 Task 1.3 的 voice/router_bridge
//! 或 CLI)在已有 task context 中显式完成:LLM 路径可用
//! `llm::client::record_llm_decompose_called` 写入 `llm_decompose_called`
//! 审计事件——不要在 `plan()` 内隐式写数据库。

use std::sync::Arc;
#[cfg(feature = "llm")]
use std::time::Instant;

use crate::error::Result;
use crate::extensions::types::ExtensionSnapshot;
use crate::kernel::TrustKernel;
use crate::llm::types::ExtractedSlot;
use crate::skills::router::{RouteDecision, SkillRouter};

/// 输入来源(文本 / 语音)。当前不影响路由逻辑,仅由调用方记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerSource {
    Text,
    Voice,
}

/// 快照 TTL：超过此时长未规划即视为过期，调用方必须重采。
pub const SNAPSHOT_TTL_MS: u64 = 10_000;
/// 注入 LLM 的上下文块上限（字符数；中文按字计，天然保守）。
pub const CONTEXT_BUDGET_CHARS: usize = 1600;

#[derive(Debug, Clone)]
pub struct SnapshotVoice {
    /// "speech_ended" | "timeout" | "no_speech" | "text"
    pub outcome_kind: &'static str,
    pub stopped_by_vad: bool,
    pub sample_count: usize,
    /// "silero" | "energy"
    pub vad_backend: &'static str,
    /// 首个 voiced chunk 距快照时刻的毫秒数（无语音为 None）
    pub voice_started_ago_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotMemory {
    /// 已渲染摘要 "用户：… → …"，调用方保证每条 ≤200 字符、最多 3 条
    pub prev_turns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RealtimeSnapshot {
    pub taken_at: std::time::SystemTime,
    pub transcript_chars: usize,
    pub voice: SnapshotVoice,
    pub memory: SnapshotMemory,
    /// 采样时刻的 privacy_mode（仅审计，不决定注入；注入与否由调用路径保证）
    pub privacy_mode: bool,
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

impl RealtimeSnapshot {
    pub fn is_fresh_at(&self, now: std::time::SystemTime) -> bool {
        now.duration_since(self.taken_at)
            .map(|d| d.as_millis() as u64 <= SNAPSHOT_TTL_MS)
            .unwrap_or(false)
    }

    /// 渲染注入 LLM user 消息前缀的上下文块。结尾不带用户输入，
    /// 调用方拼 `format!("{}\n{}", snap.context_block(), trimmed)`。
    pub fn context_block(&self) -> String {
        let mut s = String::from(
            "【实时上下文，仅供理解意图；禁止引用其中的路径、数字、专有名词作为槽位值】\n",
        );
        s.push_str(&format!(
            "- 本轮转写长度：{}字；语音后端：{}；结束方式：{}；样本数：{}\n",
            self.transcript_chars,
            self.voice.vad_backend,
            self.voice.outcome_kind,
            self.voice.sample_count
        ));
        if !self.memory.prev_turns.is_empty() {
            s.push_str(&format!("- 上文：{}\n", self.memory.prev_turns.join(" ｜ ")));
        }
        truncate_chars(&s, CONTEXT_BUDGET_CHARS)
    }
}

/// PlannerPipeline 的输入。
#[derive(Debug, Clone)]
pub struct PlannerInput {
    pub text: String,
    pub source: PlannerSource,
    /// 实时快照；语音路径 Some（须新鲜），文本路径 None（行为与旧版一致）。
    pub snapshot: Option<RealtimeSnapshot>,
}

/// 规划结果。无 Clarification 变体:低置信度 / privacy_mode / LLM disabled /
/// HTTP 或解析失败一律落到 `Unmatched`,由任务运行层决定 UI 反馈。
#[derive(Debug, Clone)]
pub enum PlanResult {
    /// 空 / 纯空白输入。
    Empty,
    /// 命中单个 Skill(关键词或 LLM 高置信度)。
    Skill {
        extension_id: String,
        slots: Vec<ExtractedSlot>,
    },
    /// LLM 拆解出的多步 DAG 计划(已通过双层校验)。
    Dag(crate::skills::dag_types::DagPlan),
    /// 未匹配任何 Skill / DAG。
    Unmatched { text: String },
}

/// 一次 `plan()` 调用的内存轨迹。持久化由调用方在任务层完成。
#[derive(Debug, Clone)]
pub struct PlannerTrace {
    /// 构造时注入的 ExtensionSnapshot 的 snapshot_id。
    pub snapshot_id: String,
    /// 仅当实际调用过 LLM 时为 Some(模型名)。
    pub llm_model: Option<String>,
    /// 所有 LLM 调用的累计耗时(毫秒);未调 LLM 时为 0。
    pub latency_ms: u64,
    /// 仅当返回经过校验的 DAG 计划时为 Some(usage.total_tokens);classify 路径为 None。
    pub token_count: Option<u64>,
    /// 本次规划是否调用过 LLM。
    pub used_llm: bool,
}

/// 统一规划管道:关键词路由 → LLM classify → LLM DAG 拆解。
///
/// 持有 `Arc<TrustKernel>`(读 privacy_mode / llm_client)与一个不可变的
/// `ExtensionSnapshot`(规划期间不持有 catalog 锁)。
pub struct PlannerPipeline {
    // 无 `llm` feature 时规划不读 kernel,字段保留仅为契约稳定。
    #[cfg_attr(not(feature = "llm"), allow(dead_code))]
    kernel: Arc<TrustKernel>,
    snapshot: ExtensionSnapshot,
}

impl PlannerPipeline {
    pub fn new(kernel: Arc<TrustKernel>, snapshot: ExtensionSnapshot) -> Self {
        Self { kernel, snapshot }
    }

    /// 执行纯规划,固定顺序:
    ///
    /// 1. trimmed 空检查 → `Empty`
    /// 2. 取 snapshot 的启用候选(`candidate_manifests`)
    /// 3. 关键词 / intent-example 匹配(`SkillRouter::register` + `route`)→ `Skill`
    /// 4. 未命中且 LLM 启用 + `!privacy_mode` 时 `classify_and_extract`;
    ///    高置信度(≥ 0.7)且 skill_id 在候选内 → `Skill{extension_id, slots}`
    /// 5. 否则 `decompose_to_dag_traced`(candidate_manifests + 空 user_slots)→
    ///    双层防御校验:`SlotTemplateEngine::validate_dag` +
    ///    `DagPlan::validate_edges` / `validate_loop_specs`;通过 → `Dag`
    /// 6. 所有 LLM 失败 / 低置信度 / privacy_mode / disabled → `Unmatched`
    ///
    /// 无 DB 副作用:不创建 task、不写 audit、不写 taint、不执行工具。
    /// trace 的持久化由调用方在任务层完成(见模块文档)。
    pub async fn plan(&self, input: PlannerInput) -> Result<(PlanResult, PlannerTrace)> {
        let trimmed = input.text.trim();
        if trimmed.is_empty() {
            return Ok((PlanResult::Empty, self.trace()));
        }

        if let Some(snap) = &input.snapshot {
            if !snap.is_fresh_at(std::time::SystemTime::now()) {
                return Err(crate::error::KernelError::Skill(
                    "stale realtime snapshot: re-sense before planning".to_string(),
                ));
            }
        }

        // 2. 启用候选(snapshot 不可变,纯内存)。
        let manifests = self.snapshot.candidate_manifests();

        // 3. 关键词 / intent-example 匹配(同步,不调 LLM)。
        let mut router = SkillRouter::new();
        for manifest in &manifests {
            router.register(manifest.clone());
        }
        match router.route(trimmed) {
            RouteDecision::Skill(manifest) => {
                return Ok((
                    PlanResult::Skill {
                        extension_id: manifest.id,
                        slots: Vec::new(),
                    },
                    self.trace(),
                ));
            }
            RouteDecision::Planner => {}
            // 同步 route() 从不返回这两个变体;防御性落到 LLM 路径。
            #[cfg(feature = "llm")]
            RouteDecision::SkillWithSlots(..) | RouteDecision::Dag(_) => {}
        }

        self.plan_with_llm(trimmed, &manifests, input.snapshot.as_ref()).await
    }

    /// 关键词未命中后的 LLM 路径(4-6 步)。仅 `llm` feature 下编译。
    #[cfg(feature = "llm")]
    async fn plan_with_llm(
        &self,
        trimmed: &str,
        manifests: &[crate::skills::manifest::SkillManifest],
        snapshot: Option<&RealtimeSnapshot>,
    ) -> Result<(PlanResult, PlannerTrace)> {
        let mut trace = self.trace();

        let Some(llm) = self.kernel.llm_client() else {
            return Ok((PlanResult::Unmatched { text: trimmed.to_string() }, trace));
        };
        if !llm.is_enabled() || self.kernel.privacy_mode() {
            return Ok((PlanResult::Unmatched { text: trimmed.to_string() }, trace));
        }

        // 快照上下文只进 LLM（关键词路由仍用原文，避免污染匹配）。
        let llm_text = match snapshot {
            Some(s) => format!("{}\n{}", s.context_block(), trimmed),
            None => trimmed.to_string(),
        };

        // 4. classify_and_extract — 单 Skill 意图 + Slot 提取。
        // Task 6: classify 缓存。命中且候选仍有效 → 零 LLM 开销直接返回。
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cache_key = crate::llm_cache::route_cache_key(
            llm.model(),
            crate::llm::client::LlmClient::ROUTE_TOOL_SCHEMA_VERSION,
            &llm_text,
        );
        let cached_hit = self.kernel.lookup_route_cache(&cache_key, now_ms).unwrap_or(None);
        if let Some(hit) = cached_hit {
            if self.snapshot.resolve_candidate(&hit.skill_id).is_some() {
                let slots = serde_json::from_str(&hit.slots_json).unwrap_or_default();
                return Ok((
                    PlanResult::Skill {
                        extension_id: hit.skill_id,
                        slots,
                    },
                    trace,
                ));
            }
            // 候选已变（如 skill 下线）：当 miss 继续走 LLM。
        }
        let started = Instant::now();
        let classify = llm.classify_and_extract(&llm_text, manifests).await;
        trace.latency_ms = started.elapsed().as_millis() as u64;
        trace.used_llm = true;
        trace.llm_model = Some(llm.model().to_string());

        match classify {
            Ok(resp) if resp.confidence >= 0.7 => {
                if let Some(skill_id) = &resp.matched_skill_id {
                    // 只接受候选内(启用 + 有执行 target)的 skill_id。
                    if self.snapshot.resolve_candidate(skill_id).is_some() {
                        let slots_json = serde_json::to_string(&resp.slots)
                            .unwrap_or_else(|_| "[]".to_string());
                        let _ = self.kernel.record_route_cache(
                            &cache_key,
                            skill_id,
                            &slots_json,
                            resp.confidence,
                            now_ms,
                        );
                        return Ok((
                            PlanResult::Skill {
                                extension_id: skill_id.clone(),
                                slots: resp.slots,
                            },
                            trace,
                        ));
                    }
                }
            }
            _ => {} // 低置信度 / HTTP / 解析失败 → 继续 DAG 拆解
        }

        // 5. decompose_to_dag_traced — 多步意图拆解(空 user_slots)。
        let started = Instant::now();
        let user_slots: Vec<ExtractedSlot> = Vec::new();
        // 失败路径(HTTP / 解析 / 校验)收敛到 Unmatched。
        if let Ok((dag, stats)) = llm
            .decompose_to_dag_traced(&llm_text, manifests, &user_slots)
            .await
        {
            trace.latency_ms += started.elapsed().as_millis() as u64;

            // 双层防御校验(与 router_bridge 一致):
            // 校验 #1 — 模板语法 + 引用合法性。
            let template_ok = crate::skills::template::SlotTemplateEngine::validate_dag(&dag)
                .is_ok();
            // 校验 #2 — 边引用 + 循环规格(结构校验)。
            let structure_ok = dag.validate_edges().is_ok() && dag.validate_loop_specs().is_ok();
            if template_ok && structure_ok {
                trace.token_count = Some(stats.token_count as u64);
                return Ok((PlanResult::Dag(dag), trace));
            }
        }

        // 6. 全部失败路径收敛到 Unmatched。
        Ok((PlanResult::Unmatched { text: trimmed.to_string() }, trace))
    }

    /// 无 `llm` feature 时的回退:关键词未命中直接 Unmatched(不调 LLM)。
    #[cfg(not(feature = "llm"))]
    async fn plan_with_llm(
        &self,
        trimmed: &str,
        _manifests: &[crate::skills::manifest::SkillManifest],
        _snapshot: Option<&RealtimeSnapshot>,
    ) -> Result<(PlanResult, PlannerTrace)> {
        Ok((PlanResult::Unmatched { text: trimmed.to_string() }, self.trace()))
    }

    fn trace(&self) -> PlannerTrace {
        PlannerTrace {
            snapshot_id: self.snapshot.snapshot_id().to_string(),
            llm_model: None,
            latency_ms: 0,
            token_count: None,
            used_llm: false,
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn fresh_snapshot() -> RealtimeSnapshot {
        RealtimeSnapshot {
            taken_at: SystemTime::now(),
            transcript_chars: 12,
            voice: SnapshotVoice {
                outcome_kind: "speech_ended",
                stopped_by_vad: true,
                sample_count: 80000,
                vad_backend: "silero",
                voice_started_ago_ms: Some(1200),
            },
            memory: SnapshotMemory {
                prev_turns: vec!["用户：打开记事本 → routed:quick.app_control".to_string()],
            },
            privacy_mode: false,
        }
    }

    #[test]
    fn fresh_snapshot_passes_and_stale_fails() {
        let snap = fresh_snapshot();
        assert!(snap.is_fresh_at(SystemTime::now()));
        let old = RealtimeSnapshot {
            taken_at: SystemTime::now() - Duration::from_millis(SNAPSHOT_TTL_MS + 1000),
            ..fresh_snapshot()
        };
        assert!(!old.is_fresh_at(SystemTime::now()));
    }

    #[test]
    fn context_block_contains_turns_and_respects_budget() {
        let block = fresh_snapshot().context_block();
        assert!(block.contains("上文"));
        assert!(block.contains("打开记事本"));
        assert!(block.contains("禁止引用"));
        assert!(block.chars().count() <= CONTEXT_BUDGET_CHARS);
    }

    #[test]
    fn context_block_empty_memory_has_no_prev_line() {
        let mut snap = fresh_snapshot();
        snap.memory.prev_turns.clear();
        assert!(!snap.context_block().contains("上文"));
    }
}
