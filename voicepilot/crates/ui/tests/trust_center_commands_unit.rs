#![cfg(feature = "tauri")]

//! trust_center_commands 单元测试 —— MCP Plugin 配置入口(register/remove/
//! list/toggle 逻辑函数,Wave 2 Task 2.2)。
//!
//! TDD 约定:逻辑函数(`register_mcp_server` / `remove_mcp_server` /
//! `list_mcp_servers` / `toggle_mcp_server`)不依赖 Tauri 运行时,
//! 直接用 `AppState` 调用,供测试验证 command 边界行为。

use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::McpServerRecord;
use voicepilot_ui::state::AppState;
use voicepilot_ui::trust_center_commands::{
    import_external_mcp, list_mcp_servers, register_mcp_server, remove_mcp_server,
    scan_external_mcp, toggle_mcp_server, McpServerDto,
};

fn dto(server_id: &str) -> McpServerDto {
    McpServerDto {
        server_id: server_id.to_string(),
        name: format!("DTO {server_id}"),
        version: "1.0.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["D:/dto"]"#.to_string()),
        command: Some("npx".to_string()),
        args: Some(r#"["-y","@fixture/dto-mcp"]"#.to_string()),
        env: Some(r#"{"DTO_TOKEN":"dto-secret"}"#.to_string()),
    }
}

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

#[test]
fn mcp_server_dto_roundtrips_command_args_env_into_record() {
    let dto = dto("srv-full");
    let rec = dto.clone().into_record();
    assert_eq!(rec.server_id, "srv-full");
    assert_eq!(rec.command.as_deref(), Some("npx"));
    assert_eq!(rec.args.as_deref(), Some(r#"["-y","@fixture/dto-mcp"]"#));
    assert_eq!(rec.env.as_deref(), Some(r#"{"DTO_TOKEN":"dto-secret"}"#));
    assert_eq!(rec.allowed_paths.as_deref(), Some(r#"["D:/dto"]"#));
    assert!(rec.enabled && rec.trusted);
}

#[test]
fn register_command_logic_rejects_invalid_and_accepts_valid() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    let before = list_mcp_servers(&state).expect("list").len();

    let mut invalid = dto("srv-invalid");
    invalid.transport = "http".to_string();
    let err = register_mcp_server(&state, invalid)
        .expect_err("invalid transport must be rejected at the command boundary");
    assert!(
        err.to_string().to_ascii_lowercase().contains("transport"),
        "command error should mention transport: {err}"
    );
    assert_eq!(
        list_mcp_servers(&state).expect("list").len(),
        before,
        "failed register must not change the DB"
    );

    register_mcp_server(&state, dto("srv-valid")).expect("valid register must succeed");
    assert!(
        list_mcp_servers(&state)
            .expect("list")
            .iter()
            .any(|s| s.server_id == "srv-valid"),
        "registered server must appear in the list"
    );
}

#[test]
fn register_remove_toggle_command_logic_roundtrip() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);

    register_mcp_server(&state, dto("srv-1")).expect("register");

    toggle_mcp_server(&state, "srv-1", false).expect("toggle off");
    let s = list_mcp_servers(&state)
        .expect("list")
        .into_iter()
        .find(|s| s.server_id == "srv-1")
        .expect("row must exist");
    assert!(!s.enabled, "toggle off must be reflected in the DTO list");

    toggle_mcp_server(&state, "srv-1", true).expect("toggle on");
    let s = list_mcp_servers(&state)
        .expect("list")
        .into_iter()
        .find(|s| s.server_id == "srv-1")
        .expect("row must exist");
    assert!(s.enabled, "toggle on must be reflected in the DTO list");

    remove_mcp_server(&state, "srv-1").expect("remove unreferenced server");
    assert!(
        list_mcp_servers(&state)
            .expect("list")
            .iter()
            .all(|s| s.server_id != "srv-1"),
        "removed server must disappear from the list"
    );
}

// ===== 主流标准 MCP JSON 兼容(2026-08-24 统一) =====

#[test]
fn import_mcp_servers_parses_standard_mcp_servers_json() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    let json = r#"{
        "mcpServers": {
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "D:/docs"],
                "env": { "TOKEN": "secret" }
            }
        }
    }"#;
    let result = voicepilot_ui::trust_center_commands::import_mcp_servers(&state, json)
        .expect("import must succeed");
    assert_eq!(result.imported, 1);
    assert!(result.errors.is_empty());

    let servers = list_mcp_servers(&state).expect("list");
    // in-memory kernel 自带预置 playwright server,断言目标条目存在即可
    let s = servers
        .iter()
        .find(|s| s.server_id == "filesystem")
        .expect("filesystem must be imported");
    assert_eq!(s.command.as_deref(), Some("npx"));
    // args/env 归一化为边界 JSON 字符串
    assert_eq!(
        s.args.as_deref(),
        Some(r#"["-y","@modelcontextprotocol/server-filesystem","D:/docs"]"#)
    );
    // Phase A：TOKEN 为疑似凭据，只存 keyring 引用，明文值不落库。
    let env = s.env.as_deref().unwrap();
    assert!(env.contains("TOKEN"));
    assert!(
        env.contains("$keyring"),
        "credential env must be keyring-referenced, got {env}"
    );
    assert!(
        !env.contains("secret"),
        "credential value must not be stored plaintext, got {env}"
    );
    // 安全默认:导入后不可执行,须 UI 显式标 trusted
    assert!(!s.trusted, "imported server must default to untrusted");
    assert!(s.enabled);
}

#[test]
fn import_mcp_servers_accepts_vscode_servers_root_and_reports_unsupported_transport() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    // VS Code 变体:根键 servers + 一个 http 类型(本期不支持,逐条报错)
    let json = r#"{
        "servers": {
            "good": { "type": "stdio", "command": "python", "args": ["srv.py"] },
            "remote": { "type": "http", "url": "https://mcp.example.com/mcp" }
        }
    }"#;
    let result = voicepilot_ui::trust_center_commands::import_mcp_servers(&state, json)
        .expect("import must succeed");
    assert_eq!(result.imported, 1, "只有 stdio 条目被导入");
    assert_eq!(result.errors.len(), 1, "http 条目必须逐条报错");
    assert_eq!(result.errors[0].name, "remote");
    assert!(result.errors[0].error.contains("http"));
}

#[test]
fn import_mcp_servers_reports_missing_command_and_duplicates() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    let json = r#"{
        "mcpServers": {
            "no-cmd": { "args": ["-y"] },
            "dup": { "command": "npx", "args": ["a"] }
        }
    }"#;
    let result = voicepilot_ui::trust_center_commands::import_mcp_servers(&state, json)
        .expect("import must succeed");
    assert_eq!(result.imported, 1, "只有 dup 合法");
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].name, "no-cmd");
    assert!(result.errors[0].error.contains("command"));

    // 重复导入同 id → 单条报错(register 校验重复)
    let dup = r#"{"mcpServers": {"dup": { "command": "npx" }}}"#;
    let result2 = voicepilot_ui::trust_center_commands::import_mcp_servers(&state, dup)
        .expect("import must succeed");
    assert_eq!(result2.imported, 0);
    assert!(!result2.errors.is_empty());
}

#[test]
fn export_mcp_servers_produces_standard_json_without_private_fields() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    register_mcp_server(
        &state,
        McpServerDto {
            server_id: "exp-server".to_string(),
            name: "Exp".to_string(),
            version: "1.0.0".to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            trusted: true,
            protocol_version: Some("2025-11-25".to_string()),
            allowed_origins: None,
            allowed_paths: Some(r#"["D:/secret"]"#.to_string()),
            command: Some("npx".to_string()),
            args: Some(r#"["-y","@fixture/mcp"]"#.to_string()),
            env: None,
        },
    )
    .expect("register");

    let out = voicepilot_ui::trust_center_commands::export_mcp_servers(&state).expect("export");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    let servers = v
        .get("mcpServers")
        .expect("mcpServers root")
        .as_object()
        .expect("object");
    let entry = servers
        .get("exp-server")
        .expect("server entry")
        .as_object()
        .expect("object");
    assert_eq!(entry.get("command").and_then(|x| x.as_str()), Some("npx"));
    assert_eq!(
        entry.get("args"),
        Some(&serde_json::json!(["-y", "@fixture/mcp"])),
        "args 输出为 JSON 数组而非字符串"
    );
    // 私有字段不导出
    for k in [
        "trusted",
        "enabled",
        "allowed_paths",
        "allowed_origins",
        "protocol_version",
        "version",
        "server_id",
    ] {
        assert!(
            !entry.contains_key(k),
            "private field {k} must not be exported"
        );
    }
}

#[test]
fn scan_external_mcp_runs_without_error() {
    // 只读扫描：真实机器上可能命中也可能为空，断言仅为“不炸且形状合法”。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    let report = scan_external_mcp(&state).expect("scan ok");
    for h in &report.hits {
        assert!(!h.server_id.is_empty());
        assert!(!h.command.trim().is_empty());
        assert!(!h.source_file.is_empty());
    }
}

#[test]
fn scan_external_mcp_with_files_maps_every_field() {
    // TempDir 固件逐字段断言映射：交换任两字段即变红。
    use voicepilot_ui::trust_center_commands::scan_external_mcp_with_files;
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = tmp.path().join("claude.json");
    std::fs::write(
        &cfg,
        r#"{"mcpServers":{"fx": {"command": "npx", "args": ["-y"], "env": {"K": "v"}}}}"#,
    )
    .expect("write");
    let report = scan_external_mcp_with_files(&[(
        cfg,
        trust_kernel::external_scan::McpConfigFormat::ClaudeDesktop,
    )]);
    assert_eq!(report.hits.len(), 1);
    let h = &report.hits[0];
    assert_eq!(h.server_id, "fx");
    assert_eq!(h.format, "claude-desktop");
    assert_eq!(h.command, "npx");
    assert_eq!(h.args, vec!["-y".to_string()]);
    assert_eq!(h.env_keys, vec!["K".to_string()]);
    assert!(report.skipped.is_empty());
}

#[test]
fn import_external_mcp_matches_manual_import_posture() {
    // 与手动粘贴导入完全一致：可见可管理(enabled)，不可被执行(!trusted)。
    // 按引用导入：env 值由后端从源文件重读，前端只传回 (file, format, id)。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let state = AppState::new(kernel);
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = tmp.path().join("claude.json");
    std::fs::write(
        &cfg,
        r#"{"mcpServers":{"ext-probe": {"command": "npx", "args": ["-y", "probe"], "env": {"TOK": "s3cr3t"}}}}"#,
    )
    .expect("write");
    let cfg_str = cfg.to_string_lossy().into_owned();
    import_external_mcp(&state, &cfg_str, "claude-desktop", "ext-probe").expect("import ok");
    let s = list_mcp_servers(&state)
        .expect("list")
        .into_iter()
        .find(|s| s.server_id == "ext-probe")
        .expect("row must exist");
    assert!(s.enabled);
    assert!(!s.trusted, "imported server must default to untrusted");
    assert_eq!(s.command.as_deref(), Some("npx"));
    // args/env 落库形状与手动导入一致：JSON 数组字符串 / JSON 对象字符串。
    assert_eq!(s.args.as_deref(), Some(r#"["-y","probe"]"#));
    assert_eq!(s.env.as_deref(), Some(r#"{"TOK":"s3cr3t"}"#));
    // 重复导入显式拒绝（register 校验）。
    assert!(import_external_mcp(&state, &cfg_str, "claude-desktop", "ext-probe").is_err());
    // 源文件里没有的 id：显式拒绝（防前端伪造引用）。
    assert!(import_external_mcp(&state, &cfg_str, "claude-desktop", "ghost").is_err());
    // 未知格式：显式拒绝。
    assert!(import_external_mcp(&state, &cfg_str, "nope", "ext-probe").is_err());
}
