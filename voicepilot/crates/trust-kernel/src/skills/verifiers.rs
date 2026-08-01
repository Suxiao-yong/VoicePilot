//! Strong Verifier 函数集合 — W10 Plan 1。
//!
//! 为 6 个有副作用的 Skill 提供真实 Strong Verifier 实现:
//! - verify_note_capture: 重读 note 文件存在 + sha256 + size 匹配 content
//! - verify_research_save: 重读 markdown 文件存在 + size > 0
//! - verify_form_prepare: Playwright 重查表单字段值匹配 effect_manifest.fields
//! - verify_form_submit: Playwright 查页面 URL 变更 或 success 元素存在
//! - verify_task_repeat: 同 files.organize.verify_move,重读目标文件 sha256+size
//! - verify_task_compensate: 查 compensations 表 status=reversed + reverse_payload 非空
//!
//! files.organize 已实现(FilesystemTool::verify_move),不在此模块。
//! task.explain 是只读 Skill,verifier.strategy="none",不在此模块。
//!
//! spec §6.3 "Verifier 读取真实状态":每个 verify 函数必须在 commit 之后
//! 重新读取真实世界状态(filesystem / DB / Playwright page),而非依赖
//! executor 内部状态。Strong = 真实 artifact 验证(sha256 / DB 记录 / 页面元素)。

use crate::error::Result;
use crate::kernel::TrustKernel;
use serde_json::Value;

/// Verifier 调用上下文。`step_id` 与现有 executor 的 String step_id 一致。
pub struct VerificationContext<'a> {
    pub kernel: &'a TrustKernel,
    pub step_id: &'a str,
}

/// Verifier 输出。Strong = 真实 artifact 验证通过;Failed = 验证失败
/// (文件不存在 / sha256 不匹配 / DB 记录不存在 / 页面元素缺失)。
/// Medium / Weak 在 Plan 1 不使用,保留枚举变体供未来扩展。
#[derive(Debug, Clone)]
pub enum VerificationOutcome {
    Strong { evidence: Value },
    #[allow(dead_code)]
    Medium { evidence: Value },
    #[allow(dead_code)]
    Weak { reason: String },
    Failed { reason: String },
}

impl VerificationOutcome {
    /// 提取 evidence_strength 字符串(供 finalize_step_success 使用)。
    pub fn evidence_strength(&self) -> &'static str {
        match self {
            VerificationOutcome::Strong { .. } => "strong",
            VerificationOutcome::Medium { .. } => "medium",
            VerificationOutcome::Weak { .. } => "weak",
            VerificationOutcome::Failed { .. } => "weak",
        }
    }
}

/// verify_note_capture — 重读 save_path 文件存在 + sha256 匹配 content。
pub fn verify_note_capture(
    _ctx: &VerificationContext<'_>,
    _save_path: &str,
    _expected_content: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 2 implements verify_note_capture")
}

/// verify_research_save — 重读 save_path 文件存在 + size > 0。
pub fn verify_research_save(
    _ctx: &VerificationContext<'_>,
    _save_path: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 3 implements verify_research_save")
}

/// verify_form_prepare — Playwright 重查表单字段值匹配 fields。
pub fn verify_form_prepare(
    _ctx: &VerificationContext<'_>,
    _fields: &std::collections::HashMap<String, String>,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 4 implements verify_form_prepare")
}

/// verify_form_submit — Playwright 查页面 URL 变更 或 success 元素存在。
pub fn verify_form_submit(
    _ctx: &VerificationContext<'_>,
    _submitted_url: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 5 implements verify_form_submit")
}

/// verify_task_repeat — 同 files.organize.verify_move,重读目标文件 sha256+size。
pub fn verify_task_repeat(
    _ctx: &VerificationContext<'_>,
    _target_task_id: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 6 implements verify_task_repeat")
}

/// verify_task_compensate — 查 compensations 表 status=reversed + reverse_payload 非空。
pub fn verify_task_compensate(
    _ctx: &VerificationContext<'_>,
    _target_step_id: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 7 implements verify_task_compensate")
}

#[cfg(test)]
mod tests {
    // 单元测试在 Task 2-7 各自添加。
}
