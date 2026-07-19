//! Egress policy — V1.1 §4.3.
//! Controls data flow from local sources to remote destinations.
//! Independent from E×D matrix; checked separately per §4.4 step 6.

use crate::policy::types::{DLevel, Effect, EgressDest};

/// Check whether data of `data_class` may flow to `dest`.
///
/// V1.1 §4.3 table:
///   local_file → remote_llm:  D0/D1 allow, D2 confirm, D3 deny
///   local_file → remote_mcp:  D0/D1 allow, D2 confirm, D3 deny
///   web_page → local_file:    allow (caller tags taint=external_untrusted)
///   web_page → remote_llm:    allow (read-only context)
///   web_page → tool_argument: deny (taint cannot elevate to instruction)
///
/// W2 simplification: `data_class` is the local data's classification.
/// `web_page` provenance is handled by checking `EgressDest::ToolArgument`
/// — any data flowing into a tool argument is denied because taint may
/// not elevate to an instruction. Full provenance-aware egress lands in W7
/// when Taint Tracking is implemented.
pub fn check_egress(data_class: DLevel, dest: EgressDest) -> Effect {
    use DLevel::*;
    use EgressDest::*;
    use Effect::*;
    match (data_class, dest) {
        // Tool arguments are never allowed to carry data (taint elevation).
        (_, ToolArgument) => Deny,
        // Local file destination: always allow (it's a local write, E×D covers it).
        (_, LocalFile) => Allow,
        // Remote LLM / MCP
        (D0 | D1, RemoteLlm | RemoteMcp) => Allow,
        (D2, RemoteLlm | RemoteMcp) => Confirm,
        (D3, RemoteLlm | RemoteMcp) => Deny,
    }
}
