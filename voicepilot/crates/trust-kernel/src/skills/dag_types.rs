//! W8 DAG 编排核心数据结构 — V1.1.2 §5.4 + §6.2.
//!
//! 本文件仅含数据结构(serde 序列化),无业务逻辑。
//! Plan 2 的 DagExecutor 引用这里的 DagPlan / DagNode / DagStatus。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::policy::types::ELevel;
use crate::skills::manifest::SkillManifest;
use crate::skills::template::SlotTemplate;

/// LLM 拆解生成的 DAG 计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlan {
    /// uuid v4
    pub plan_id: String,
    /// 原始语音转写文本
    pub user_goal: String,
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
    /// key = node_id(循环节点)
    pub loop_specs: HashMap<String, LoopSpec>,
    /// 全局上限,硬约束 ≤ 20
    pub max_total_steps: u32,
}

/// DAG 节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    /// "n1" / "n2"
    pub node_id: String,
    /// "note.capture" / "files.move"
    pub skill_id: String,
    pub input_template: SlotTemplate,
    pub risk_ceiling: ELevel,
}

/// DAG 边。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
    /// 端口绑定(如 "output.path" → "input.source"),W8 暂不强制校验,仅记录
    pub port_binding: Option<String>,
}

/// 循环节点的循环规格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSpec {
    /// 循环变量,如 "item"
    pub loop_var: String,
    pub iterable_source: IterableSource,
    /// 硬上限 50,运行时强制 `min(spec.max_iterations, 50)`
    pub max_iterations: u32,
    /// 中断条件(简单表达式,如 "item.size > 1048576"),W8 仅支持简单比较
    pub break_condition: Option<String>,
}

/// 可迭代来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IterableSource {
    /// 上游节点的 `output.<port>` 是数组
    PrevNodeOutput { node_id: String, port: String },
    /// 用户审批阶段填的 Slot(如 Files list)
    UserSlot { slot_kind: String },
    /// 字面量数组(用于测试 / 简单场景)
    Literal(Vec<String>),
}

/// DAG 整体状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DagStatus {
    Pending,
    Running,
    /// W10 Plan 4: Kill Switch 中间态(spec §6.2 v2 修订 #9)。
    ///
    /// 语义:DAG 进入 Cancelling 时,正在执行的 node 等待完成(不主动中断),
    /// 未启动 node 跳过。node 完成后 DAG → Cancelled。
    /// DAG 级不强制 1s SLA(node 执行时长可能 > 1s)。
    Cancelling,
    Succeeded,
    Failed { failed_node: String, cause: String },
    /// 决策 #8:循环失败时,若有成功节点 → PartiallySucceeded
    PartiallySucceeded {
        succeeded: Vec<String>,
        failed_node: String,
        cause: String,
    },
    Cancelled,
}

impl DagStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::PartiallySucceeded { .. } => "partially_succeeded",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed {
                failed_node: String::new(),
                cause: String::new(),
            }),
            "partially_succeeded" => Some(Self::PartiallySucceeded {
                succeeded: Vec::new(),
                failed_node: String::new(),
                cause: String::new(),
            }),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// W9 Plan 7 + W10 Plan 4: 检查从 from 到 to 的状态转换是否合法(spec §2.7 状态机)。
    ///
    /// 合法转换:
    /// - Pending → Running
    /// - Running → Succeeded
    /// - Running → Failed
    /// - Running → Cancelled(直跳,向后兼容)
    /// - Running → Cancelling(W10 Plan 4 新增)
    /// - Cancelling → Cancelled(W10 Plan 4 新增)
    /// - Running → PartiallySucceeded
    ///
    /// 终态(Succeeded / Failed / PartiallySucceeded / Cancelled)不可逆。
    /// 返回 true=合法,false=非法。
    pub fn transition(from: &DagStatus, to: &DagStatus) -> bool {
        use DagStatus::*;
        matches!(
            (from, to),
            (Pending, Running)
                | (Running, Succeeded)
                | (Running, Failed { .. })
                | (Running, Cancelled)
                | (Running, Cancelling)
                | (Cancelling, Cancelled)
                | (Running, PartiallySucceeded { .. })
        )
    }
}

/// DAG 节点执行状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DagNodeStatus {
    Pending,
    Running,
    /// 成功,output 存 JSON value
    Succeeded(serde_json::Value),
    Failed { cause: String },
    /// 条件分支未命中(决策 #5)
    Skipped,
}

impl DagNodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded(_) => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// DagExecutor::run 的返回值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagResult {
    pub status: DagStatus,
    /// key = node_id
    pub node_results: HashMap<String, DagNodeStatus>,
}

impl DagResult {
    pub fn cancelled() -> Self {
        Self {
            status: DagStatus::Cancelled,
            node_results: HashMap::new(),
        }
    }

    pub fn succeeded(node_results: HashMap<String, DagNodeStatus>) -> Self {
        Self {
            status: DagStatus::Succeeded,
            node_results,
        }
    }
}

/// DagPlan 的硬上限(spec §6 安全约束)。
pub const MAX_TOTAL_STEPS_HARD_LIMIT: u32 = 20;
/// LoopSpec::max_iterations 的硬上限。
pub const MAX_LOOP_ITERATIONS_HARD_LIMIT: u32 = 50;

impl DagPlan {
    /// 校验:全局上限 ≤ 20。
    pub fn validate_total_steps(&self) -> Result<(), String> {
        if self.max_total_steps > MAX_TOTAL_STEPS_HARD_LIMIT {
            return Err(format!(
                "max_total_steps {} exceeds hard limit {}",
                self.max_total_steps, MAX_TOTAL_STEPS_HARD_LIMIT
            ));
        }
        Ok(())
    }

    /// 校验:所有 LoopSpec 的 max_iterations ≤ 50。
    pub fn validate_loop_iterations(&self) -> Result<(), String> {
        for (node_id, spec) in &self.loop_specs {
            if spec.max_iterations > MAX_LOOP_ITERATIONS_HARD_LIMIT {
                return Err(format!(
                    "loop_spec[{}] max_iterations {} exceeds hard limit {}",
                    node_id, spec.max_iterations, MAX_LOOP_ITERATIONS_HARD_LIMIT
                ));
            }
        }
        Ok(())
    }

    /// 校验:所有 edge 的 from / to 必须在 nodes 中存在。
    pub fn validate_edges(&self) -> Result<(), String> {
        let node_ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.node_id.as_str()).collect();
        for edge in &self.edges {
            if !node_ids.contains(edge.from.as_str()) {
                return Err(format!("edge.from {} not in nodes", edge.from));
            }
            if !node_ids.contains(edge.to.as_str()) {
                return Err(format!("edge.to {} not in nodes", edge.to));
            }
        }
        Ok(())
    }

    /// 校验:所有 loop_specs 的 key 必须在 nodes 中存在。
    pub fn validate_loop_specs(&self) -> Result<(), String> {
        let node_ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.node_id.as_str()).collect();
        for node_id in self.loop_specs.keys() {
            if !node_ids.contains(node_id.as_str()) {
                return Err(format!("loop_spec key {} not in nodes", node_id));
            }
        }
        Ok(())
    }

    /// W9 Plan 7: 聚合校验 — 空 DAG 不合法 + 所有分项校验通过。
    ///
    /// spec §2.7 边界:空 nodes 的 DagPlan 不合法(至少需要一个节点)。
    /// 分项校验:validate_total_steps + validate_loop_iterations +
    /// validate_edges + validate_loop_specs。
    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.is_empty() {
            return Err("DagPlan nodes is empty: empty DAG is not allowed".to_string());
        }
        self.validate_total_steps()?;
        self.validate_loop_iterations()?;
        self.validate_edges()?;
        self.validate_loop_specs()?;
        Ok(())
    }
}

/// 辅助:从 SkillManifest 列表查找指定 skill_id 的 manifest。
pub fn find_manifest<'a>(
    skills: &'a [SkillManifest],
    skill_id: &str,
) -> Option<&'a SkillManifest> {
    skills.iter().find(|s| s.id == skill_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::template::{SlotKind, TemplateExpr};

    fn dummy_template() -> SlotTemplate {
        SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("test".into()),
        }
    }

    fn dummy_node(id: &str, skill: &str) -> DagNode {
        DagNode {
            node_id: id.into(),
            skill_id: skill.into(),
            input_template: dummy_template(),
            risk_ceiling: ELevel::E1,
        }
    }

    #[test]
    fn validate_total_steps_accepts_within_limit() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        assert!(plan.validate_total_steps().is_ok());
    }

    #[test]
    fn validate_total_steps_rejects_over_20() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![],
            edges: vec![],
            loop_specs: HashMap::new(),
            max_total_steps: 21,
        };
        assert!(plan.validate_total_steps().is_err());
    }

    #[test]
    fn validate_loop_iterations_rejects_over_50() {
        let mut specs = HashMap::new();
        specs.insert(
            "n1".into(),
            LoopSpec {
                loop_var: "item".into(),
                iterable_source: IterableSource::Literal(vec!["a".into()]),
                max_iterations: 51,
                break_condition: None,
            },
        );
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: specs,
            max_total_steps: 10,
        };
        assert!(plan.validate_loop_iterations().is_err());
    }

    #[test]
    fn validate_edges_rejects_dangling_from() {
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![DagEdge {
                from: "n1".into(),
                to: "n99".into(), // 不存在
                port_binding: None,
            }],
            loop_specs: HashMap::new(),
            max_total_steps: 5,
        };
        assert!(plan.validate_edges().is_err());
    }

    #[test]
    fn validate_loop_specs_rejects_dangling_key() {
        let mut specs = HashMap::new();
        specs.insert(
            "n99".into(), // 不在 nodes 中
            LoopSpec {
                loop_var: "item".into(),
                iterable_source: IterableSource::Literal(vec!["a".into()]),
                max_iterations: 5,
                break_condition: None,
            },
        );
        let plan = DagPlan {
            plan_id: "p1".into(),
            user_goal: "test".into(),
            nodes: vec![dummy_node("n1", "note.capture")],
            edges: vec![],
            loop_specs: specs,
            max_total_steps: 5,
        };
        assert!(plan.validate_loop_specs().is_err());
    }

    #[test]
    fn dag_status_as_round_trip() {
        for s in ["pending", "running", "succeeded", "cancelled"] {
            let parsed = DagStatus::parse_str(s).unwrap();
            assert_eq!(parsed.as_str(), s);
        }
        // failed / partially_succeeded 携带 payload,as_str 仍正确
        let failed = DagStatus::Failed {
            failed_node: "n1".into(),
            cause: "err".into(),
        };
        assert_eq!(failed.as_str(), "failed");
    }

    #[test]
    fn dag_node_status_is_succeeded_failed() {
        let ok = DagNodeStatus::Succeeded(serde_json::json!({"k": "v"}));
        assert!(ok.is_succeeded());
        assert!(!ok.is_failed());

        let err = DagNodeStatus::Failed { cause: "x".into() };
        assert!(!err.is_succeeded());
        assert!(err.is_failed());

        let pending = DagNodeStatus::Pending;
        assert!(!pending.is_succeeded());
        assert!(!pending.is_failed());
    }

    #[test]
    fn find_manifest_locates_by_id() {
        // 用 files_organize manifest 测试(W3b built-in)
        let skills = vec![crate::skills::manifest::files_organize_manifest()];
        let found = find_manifest(&skills, "files.organize");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "files.organize");
        assert!(find_manifest(&skills, "nonexistent").is_none());
    }
}
