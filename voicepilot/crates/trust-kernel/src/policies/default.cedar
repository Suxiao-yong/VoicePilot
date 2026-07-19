// VoicePilot V1.1 default Cedar policy bundle — §4.2.
// Cedar produces binary allow/deny; Rust Constraint Engine upgrades to ternary.

// L0: read public docs — allow
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D0" && resource.path like "/docs/**" };

// D2 private doc read — Cedar permits, Rust Constraint upgrades to confirm
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D2" };

// D3 credentials — hard forbid (red line)
forbid (
    principal,
    action,
    resource
)
when { resource.data_class == "D3" };

// Egress to remote LLM — D0/D1 only; D2/D3 forbidden in Cedar
permit (
    principal == User::"voicepilot",
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D0" || resource.data_class == "D1" };

forbid (
    principal,
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D2" || resource.data_class == "D3" };

// Move files — Cedar permits; Rust Constraint applies max_files/overwrite
permit (
    principal == User::"voicepilot",
    action == Action::"move_files",
    resource
)
when { resource.data_class != "D3" };

// Shell — always forbidden
forbid (
    principal,
    action == Action::"shell_exec",
    resource
);
