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
fn server_dispatch_returns_response_for_known_method_placeholder() {
    // initialize is added in Task 7; for now we test that the dispatch
    // path produces a Response (not an Error) when the method exists.
    // This test will be replaced in Task 7.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    // Task 6 placeholder: initialize returns MethodNotFound (no handler yet).
    // Task 7 will flip this to a real Response.
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(s.contains(r#""code":-32601"#));
}
