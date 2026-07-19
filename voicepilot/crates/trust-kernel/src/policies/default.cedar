// VoicePilot V1.1 default Cedar policy bundle — §4.2.
// Cedar produces binary allow/deny; Rust Constraint Engine upgrades to ternary.
// Each policy has an explicit ID for audit traceability (V1.1 §8.1).

// L0: read public docs — allow
@id("read_public_docs")
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D0" && resource.path like "/docs/**" };

// D2 private doc read — Cedar permits, Rust Constraint upgrades to confirm
@id("read_d2_private")
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D2" };

// D3 credentials — hard forbid (red line)
@id("forbid_d3_read")
forbid (
    principal,
    action,
    resource
)
when { resource.data_class == "D3" };

// Egress to remote LLM — D0/D1 only; D2/D3 forbidden in Cedar
@id("allow_remote_llm_d0_d1")
permit (
    principal == User::"voicepilot",
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D0" || resource.data_class == "D1" };

@id("forbid_remote_llm_d2_d3")
forbid (
    principal,
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D2" || resource.data_class == "D3" };

// Move files — Cedar permits; Rust Constraint applies max_files/overwrite
@id("allow_move_files_non_d3")
permit (
    principal == User::"voicepilot",
    action == Action::"move_files",
    resource
)
when { resource.data_class != "D3" };

// Shell — always forbidden
@id("forbid_shell_exec")
forbid (
    principal,
    action == Action::"shell_exec",
    resource
);
