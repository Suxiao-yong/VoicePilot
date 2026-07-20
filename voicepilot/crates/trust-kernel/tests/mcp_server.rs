use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::server::McpServer;
use trust_kernel::mcp::transport::{JsonRpcId, JsonRpcRequest};

#[test]
fn server_dispatch_unknown_method_returns_method_not_found() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "foo/bar".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be an error response with code -32601.
    assert!(s.contains(r#""code":-32601"#));
    assert!(s.contains(r#""id":1"#));
}

#[test]
fn server_initialize_returns_protocol_2025_11_25_and_capabilities() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: Some(serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": {"name": "test-client", "version": "0.1.0"}
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be a Response (not Error).
    assert!(!s.contains(r#""code""#), "initialize must succeed, got: {}", s);
    assert!(s.contains(r#""protocolVersion":"2025-11-25""#));
    // Must advertise tools capability.
    assert!(s.contains(r#""tools""#));
    // Must include serverInfo with name + version.
    assert!(s.contains(r#""serverInfo""#));
    assert!(s.contains(r#""voicepilot""#));
}
