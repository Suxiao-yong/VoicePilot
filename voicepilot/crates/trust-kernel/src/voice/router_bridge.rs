//! RouterBridge — wires transcribed text → SkillRouter → Skill execution.
//!
//! V1.1 §2.1 + §5.1: voice input pipeline terminal stage.
//! Returns RouteOutcome so caller (CLI) can decide UI feedback.

use crate::approval::approver::Approver;
use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::skills::manifest::files_organize_manifest;
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
    // W7+: register additional built-in skills here.

    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteOutcome::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}
