use trust_kernel::policy::egress::check_egress;
use trust_kernel::policy::types::{DLevel::*, Effect::*, EgressDest};

#[test]
fn local_file_to_remote_llm_d0_d1_allowed() {
    assert_eq!(check_egress(D0, EgressDest::RemoteLlm), Allow);
    assert_eq!(check_egress(D1, EgressDest::RemoteLlm), Allow);
}

#[test]
fn local_file_to_remote_llm_d2_needs_confirm() {
    assert_eq!(check_egress(D2, EgressDest::RemoteLlm), Confirm);
}

#[test]
fn local_file_to_remote_llm_d3_denied_red_line() {
    assert_eq!(check_egress(D3, EgressDest::RemoteLlm), Deny);
}

#[test]
fn local_file_to_remote_mcp_d2_confirms_d3_denies() {
    assert_eq!(check_egress(D2, EgressDest::RemoteMcp), Confirm);
    assert_eq!(check_egress(D3, EgressDest::RemoteMcp), Deny);
}

#[test]
fn web_page_to_local_file_allowed_with_taint() {
    // web_page → local_file: allow (caller must tag taint=external_untrusted)
    assert_eq!(check_egress(D0, EgressDest::LocalFile), Allow);
}

#[test]
fn web_page_to_tool_argument_denied_taint_cannot_become_instruction() {
    // web_page → tool_argument: deny (taint cannot elevate to instruction)
    assert_eq!(check_egress(D0, EgressDest::ToolArgument), Deny);
}
