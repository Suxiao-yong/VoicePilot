//! RouterBridge — wires transcribed text → SkillRouter → Skill execution.
//!
//! V1.1 §2.1 + §5.1: voice input pipeline terminal stage.
//! Returns RouteOutcome so caller (CLI) can decide UI feedback.

use crate::approval::approver::Approver;
use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::skills::manifest::{
    files_organize_manifest, task_compensate_manifest, task_explain_manifest,
    task_repeat_verified_manifest,
};
use crate::skills::router::{RouteDecision, SkillRouter};

#[derive(Debug)]
pub enum RouteOutcome {
    /// Skill matched. Caller (CLI) prompts user for args, then invokes
    /// `FilesOrganizeSkill::execute` to run the prepare→approve→commit flow.
    Routed {
        skill_id: String,
    },
    /// No skill matched; caller should fall back to LLM Planner (W7).
    Unmatched { text: String },
    /// Input was empty/whitespace.
    Empty,
}

/// Route transcribed text through SkillRouter.
///
/// If a Skill is matched, this fn does NOT execute the Skill — execution
/// requires user-supplied arguments (source, filter, destination) that
/// aren't derivable from the voice text alone in W5. W5 PoC: caller (CLI)
/// prompts user for missing args. W7 LLM Planner will extract args from
/// text automatically.
///
/// The `_kernel` and `_approver` params are unused in W5 (routing only);
/// they exist so W7 can extend this fn to invoke the Skill executor
/// directly without changing the call signature.
pub fn route_text(
    _kernel: &TrustKernel,
    _approver: &dyn Approver,
    text: &str,
) -> Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    // 注册顺序: task_compensate 必须在 task_explain 之前,否则 task_explain 的
    // keyword "上一步" 会先匹配 "撤销上一步" / "补偿上一步" 等 compensate 查询。
    router.register(task_compensate_manifest());
    router.register(task_explain_manifest());
    // W7 Plan 4: register UIA skills (Windows-only, opt-in via `uia` feature).
    // Playwright (research_save, form_prepare) still deferred to Plan 5.
    #[cfg(all(windows, feature = "uia"))]
    {
        use crate::skills::manifest::{app_control_manifest, note_capture_manifest};
        router.register(app_control_manifest());
        router.register(note_capture_manifest());
    }

    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        // 同步 `route()` 不调用 LLM,不会产生 SkillWithSlots;若上游契约被破坏,
        // 退化为 Planner 而非 panic,保持 voice pipeline 鲁棒性。
        #[cfg(feature = "llm")]
        RouteDecision::SkillWithSlots(manifest, _slots) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteOutcome::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;

    /// W7 Plan 4 Task 6: `quick.app_control` 注册后,`route_text` 应将
    /// "打开记事本" 路由到该 Skill(intent_example 命中)。仅在 `uia` feature
    /// 开启时验证 —— 无 uia 时 manifest 未注册,路由会落到 Planner。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_app_control_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "打开记事本").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "quick.app_control"),
            "expected Routed to quick.app_control, got {:?}",
            result
        );
    }

    /// W7 Plan 4 Task 6: `note.capture` 注册后,`route_text` 应将
    /// "用记事本记录这个想法" 路由到该 Skill(intent_example "用记事本记录" 命中)。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_note_capture_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "用记事本记录这个想法").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "note.capture"),
            "expected Routed to note.capture, got {:?}",
            result
        );
    }
}
