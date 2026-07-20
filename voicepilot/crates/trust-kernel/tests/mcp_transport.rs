use trust_kernel::mcp::transport::{
    JsonRpcError, JsonRpcErrorBody, JsonRpcId, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcErrorCode,
};
use serde_json::json;

#[test]
fn request_serializes_with_camel_case_and_id() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "tools/list".to_string(),
        params: Some(json!({})),
    };
    let s = serde_json::to_string(&req).unwrap();
    assert_eq!(
        s,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
    );
}

#[test]
fn request_with_string_id_round_trips() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("abc-123".to_string()),
        method: "initialize".to_string(),
        params: Some(json!({"client": "test"})),
    };
    let s = serde_json::to_string(&req).unwrap();
    let back: JsonRpcRequest = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, JsonRpcId::String("abc-123".to_string()));
    assert_eq!(back.method, "initialize");
}

#[test]
fn notification_has_no_id() {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "notifications/initialized".to_string(),
        params: None,
    };
    let s = serde_json::to_string(&notif).unwrap();
    // Notification must not contain "id" field.
    assert!(!s.contains(r#""id""#));
    assert_eq!(
        s,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
    );
}

#[test]
fn response_with_result_round_trips() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(42),
        result: json!({"tools": []}),
    };
    let s = serde_json::to_string(&resp).unwrap();
    let back: JsonRpcResponse = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, JsonRpcId::Number(42));
    assert_eq!(back.result, json!({"tools": []}));
}

#[test]
fn error_response_uses_standard_codes() {
    let err = JsonRpcError {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        error: JsonRpcErrorBody {
            code: JsonRpcErrorCode::MethodNotFound as i32,
            message: "method not found: foo/bar".to_string(),
            data: None,
        },
    };
    let s = serde_json::to_string(&err).unwrap();
    assert!(s.contains(r#""code":-32601"#));
    assert!(s.contains(r#""message":"method not found: foo/bar""#));
}

#[test]
fn error_code_constants_match_json_rpc_spec() {
    assert_eq!(JsonRpcErrorCode::ParseError as i32, -32700);
    assert_eq!(JsonRpcErrorCode::InvalidRequest as i32, -32600);
    assert_eq!(JsonRpcErrorCode::MethodNotFound as i32, -32601);
    assert_eq!(JsonRpcErrorCode::InvalidParams as i32, -32602);
    assert_eq!(JsonRpcErrorCode::InternalError as i32, -32603);
}
