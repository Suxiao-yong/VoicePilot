//! SkillManifest — V1.1 §5.3 + Appendix C.
//!
//! Each Skill fixes: usable tools, max risk, param schema, max steps,
//! approval mode, verifier, compensation, egress. Built-in Skills are
//! Rust struct literals; user-saved Skills load from YAML in W7.

use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::policy::types::{DLevel, ELevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// W7 Plan 3: Markdown body 解析自 YAML frontmatter 之后的文本,
    /// 仅用于用户自定义 Skill(.md 文件)。built-in manifest 不设此字段(None)。
    #[serde(default)]
    pub description_body: Option<String>,
    pub intent_examples: Vec<String>,
    /// Curated routing keywords — V1.1 §5.1 Skill Router matches these
    /// against the user goal (case-insensitive substring). Authors list
    /// intentional, semantically meaningful keywords; intent_examples
    /// are demonstration sentences, not matching keywords.
    #[serde(default)]
    pub keywords: Vec<String>,
    pub inputs: HashMap<String, SkillInput>,
    pub risk_ceiling: ELevel,
    pub data_class_ceiling: DLevel,
    pub egress: EgressKind,
    pub max_steps: u32,
    pub tools: Vec<String>,
    pub approval: ApprovalConfig,
    pub compensation: CompensationConfig,
    pub verifier: VerifierConfig,
    pub failure_policy: FailurePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInput {
    pub input_type: SkillInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInputType {
    Directory,
    File,
    FileFilter,
    Text,
    Number,
    Enum,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressKind {
    LocalOnly,
    WebToLocal,
    LocalToWebDraft,
    LocalToWebSubmit,
}

impl EgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EgressKind::LocalOnly => "local_only",
            EgressKind::WebToLocal => "web_to_local",
            EgressKind::LocalToWebDraft => "local_to_web_draft",
            EgressKind::LocalToWebSubmit => "local_to_web_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    None,
    PerStep,
    BatchOnce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
    /// "prepare" | "commit" | "both"
    pub required_for: String,
    pub show_effect_manifest: bool,
    pub max_approval_scope: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompensationConfig {
    pub level: CompensationLevel,
    pub ttl_seconds: u32,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// "strong" | "medium" | "weak"
    pub strategy: String,
    pub recheck_after_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePolicy {
    pub max_retries: u32,
    pub allow_replan: bool,
    /// "stop" | "ask_user" | "compensate"
    pub on_fail: String,
}

/// The built-in files.organize Skill manifest — V1.1 §5.2 + §5.3 example.
pub fn files_organize_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "source".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Downloads".to_string(),
                "Desktop".to_string(),
                "Workspace".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "filter".to_string(),
        SkillInput {
            input_type: SkillInputType::FileFilter,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "destination".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Workspace".to_string(),
                "Documents".to_string(),
                "Desktop".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "files.organize".to_string(),
        version: "1.0.0".to_string(),
        title: "整理文件".to_string(),
        description: "搜索文件 → 生成变更清单 → 一次性批次批准 → 移动并验证 → 生成 strong Compensation".to_string(),
        description_body: None,
        intent_examples: vec![
            "把下载目录里的 PDF 移到论文文件夹".to_string(),
            "整理今天下载的文档".to_string(),
            "把下载的 PDF 整理到项目文件夹".to_string(),
        ],
        keywords: vec![
            "整理".to_string(),
            "归档".to_string(),
            "移动文件".to_string(),
            "下载目录".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 4,
        tools: vec![
            "filesystem.search_files".to_string(),
            "filesystem.prepare_move".to_string(),
            "filesystem.commit_move".to_string(),
            "filesystem.verify_move".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::BatchOnce,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 3,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 1,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

/// The built-in task.repeat_verified Skill manifest — W7 Plan 2 Task 2.
/// Re-executes the last files.organize move that passed verify_move.
pub fn task_repeat_verified_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "target_task_id".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "source_filter".to_string(),
        SkillInput {
            input_type: SkillInputType::FileFilter,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "task.repeat_verified".to_string(),
        version: "1.0.0".to_string(),
        title: "重做上一步已验证的操作".to_string(),
        description: "重新执行上一次 files.organize 中已通过 verify_move 的移动操作".to_string(),
        description_body: None,
        intent_examples: vec![
            "重做上一步".to_string(),
            "重复上次操作".to_string(),
            "再来一次上次的整理".to_string(),
        ],
        keywords: vec![
            "重做".to_string(),
            "重复".to_string(),
            "再来一次".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E1,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 2,
        tools: vec![
            "filesystem.search_files".to_string(),
            "filesystem.verify_move".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "medium".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}

/// The built-in task.explain Skill manifest — W7 Plan 2 Task 2.
/// Reads audit log and shows the most recent N operation records.
pub fn task_explain_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "limit".to_string(),
        SkillInput {
            input_type: SkillInputType::Number,
            required: false,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(100),
            default: Some(serde_json::json!(10)),
        },
    );

    SkillManifest {
        id: "task.explain".to_string(),
        version: "1.0.0".to_string(),
        title: "解释上一步操作".to_string(),
        description: "读取审计日志,展示最近 N 条操作记录与状态".to_string(),
        description_body: None,
        intent_examples: vec![
            "解释上一步".to_string(),
            "刚才做了什么".to_string(),
            "上一步做了什么".to_string(),
        ],
        keywords: vec![
            "解释".to_string(),
            "刚才".to_string(),
            "上一步".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E0,
        data_class_ceiling: DLevel::D1,
        egress: EgressKind::LocalOnly,
        max_steps: 1,
        tools: vec!["audit.read".to_string()],
        approval: ApprovalConfig {
            mode: ApprovalMode::None,
            required_for: "commit".to_string(),
            show_effect_manifest: false,
            max_approval_scope: 0,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: ConflictPolicy::AutoReverse,
        },
        verifier: VerifierConfig {
            strategy: "weak".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

/// The built-in task.compensate Skill manifest — W7 Plan 2 Task 2.
/// Runs auto_reverse compensation against a specific step.
pub fn task_compensate_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "target_step_id".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "task.compensate".to_string(),
        version: "1.0.0".to_string(),
        title: "撤销上一步操作".to_string(),
        description: "对指定 step 执行 auto_reverse 反向补偿".to_string(),
        description_body: None,
        intent_examples: vec![
            "撤销上一步".to_string(),
            "回滚刚才的操作".to_string(),
            "补偿上一步".to_string(),
        ],
        keywords: vec![
            "撤销".to_string(),
            "回滚".to_string(),
            "补偿".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 1,
        tools: vec!["compensation.auto_reverse".to_string()],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}

/// The built-in quick.app_control Skill manifest — W7 Plan 2 Task 2 stub.
/// UIA adapter implementation deferred to Plan 4.
///
/// W7 Plan 4 Task 3: added `action` input (launch|focus|close) with default
/// "launch". The `app_name` enum whitelist stays as-is — Plan 4 §2.6 lists
/// `allowed_apps` as a security property, and an enum input provides a
/// stronger whitelist than a free-form string.
pub fn app_control_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "app_name".to_string(),
        SkillInput {
            input_type: SkillInputType::Enum,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "action".to_string(),
        SkillInput {
            input_type: SkillInputType::Enum,
            required: false,
            allowed_roots: vec![],
            allowed_values: vec![
                "launch".to_string(),
                "focus".to_string(),
                "close".to_string(),
            ],
            max_length: None,
            default: Some(serde_json::json!("launch")),
        },
    );

    SkillManifest {
        id: "quick.app_control".to_string(),
        version: "1.0.0".to_string(),
        title: "控制 Windows 应用".to_string(),
        description: "启动 / 切换 / 关闭 Windows 应用(Plan 4 实现 UIA 适配器)".to_string(),
        description_body: None,
        intent_examples: vec![
            "打开记事本".to_string(),
            "切换到浏览器".to_string(),
            "关闭计算器".to_string(),
        ],
        keywords: vec![
            "打开应用".to_string(),
            "切换应用".to_string(),
            "关闭应用".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 3,
        tools: vec![
            "uiautomation.launch_app".to_string(),
            "uiautomation.find_window".to_string(),
            "uiautomation.click".to_string(),
            "uiautomation.set_text".to_string(),
            "uiautomation.get_text".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}

/// The built-in note.capture Skill manifest — W7 Plan 2 Task 2 stub.
/// UIA adapter implementation deferred to Plan 4.
pub fn note_capture_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "content".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(10000),
            default: None,
        },
    );
    inputs.insert(
        "save_path".to_string(),
        SkillInput {
            input_type: SkillInputType::File,
            required: true,
            allowed_roots: vec![
                "Documents".to_string(),
                "Desktop".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "note.capture".to_string(),
        version: "1.0.0".to_string(),
        title: "用记事本记录笔记".to_string(),
        description: "打开记事本 → 写入文本 → 保存为 .txt(Plan 4 实现 UIA 适配器)".to_string(),
        description_body: None,
        intent_examples: vec![
            "打开记事本写 TODO".to_string(),
            "记一下这个想法".to_string(),
            "用记事本记录".to_string(),
        ],
        keywords: vec![
            "记事本".to_string(),
            "记录".to_string(),
            "笔记".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 3,
        tools: vec![
            "uiautomation.launch_app".to_string(),
            "uiautomation.set_text".to_string(),
            "filesystem.write".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "medium".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}

/// The built-in research.save_markdown Skill manifest — W7 Plan 2 Task 2 stub.
/// Playwright MCP implementation deferred to Plan 5.
pub fn research_save_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "url".to_string(),
        SkillInput {
            input_type: SkillInputType::Url,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "save_path".to_string(),
        SkillInput {
            input_type: SkillInputType::File,
            required: true,
            allowed_roots: vec![
                "Documents".to_string(),
                "Desktop".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "research.save_markdown".to_string(),
        version: "1.0.0".to_string(),
        title: "把网页存为 Markdown".to_string(),
        description: "用 Playwright MCP 抓取网页内容 → 写入本地 .md 文件(Plan 5 实现)".to_string(),
        description_body: None,
        intent_examples: vec![
            "把这个网页存为 Markdown".to_string(),
            "保存这个网页内容".to_string(),
            "抓取这个网址".to_string(),
        ],
        keywords: vec![
            "网页".to_string(),
            "Markdown".to_string(),
            "抓取".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::WebToLocal,
        max_steps: 4,
        tools: vec![
            "mcp.playwright.navigate".to_string(),
            "mcp.playwright.snapshot".to_string(),
            "mcp.playwright.eval".to_string(),
            "filesystem.write".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}

/// The built-in form.prepare Skill manifest — W7 Plan 2 Task 2 stub.
/// Playwright MCP implementation deferred to Plan 5.
pub fn form_prepare_manifest() -> SkillManifest {
    let mut inputs = HashMap::new();
    inputs.insert(
        "url".to_string(),
        SkillInput {
            input_type: SkillInputType::Url,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "fields".to_string(),
        SkillInput {
            input_type: SkillInputType::Text,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: Some(5000),
            default: None,
        },
    );

    SkillManifest {
        id: "form.prepare".to_string(),
        version: "1.0.0".to_string(),
        title: "填充网页表单(不提交)".to_string(),
        description: "用 Playwright MCP 导航 → 快照 → 填充表单字段,不点击 submit(Plan 5 实现)".to_string(),
        description_body: None,
        intent_examples: vec![
            "帮我填这个表单".to_string(),
            "准备这个表单".to_string(),
            "填充网页表单".to_string(),
        ],
        keywords: vec![
            "表单".to_string(),
            "填充".to_string(),
            "准备".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalToWebDraft,
        max_steps: 3,
        tools: vec![
            "mcp.playwright.navigate".to_string(),
            "mcp.playwright.snapshot".to_string(),
            "mcp.playwright.fill".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::PerStep,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 1,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}
