#![cfg(feature = "tauri")]

use trust_kernel::mcp::repo::McpServerRecord;
use voicepilot_ui::trust_center_commands::McpServerDto;

#[test]
fn mcp_server_dto_converts_from_record() {
    let rec = McpServerRecord {
        server_id: "srv-1".to_string(),
        name: "FS".to_string(),
        version: "1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["D:/"]"#.to_string()),
        command: None,
        args: None,
        env: None,
    };
    let dto = McpServerDto::from(rec);
    assert_eq!(dto.server_id, "srv-1");
    assert!(dto.enabled);
    assert!(!dto.trusted);
}
