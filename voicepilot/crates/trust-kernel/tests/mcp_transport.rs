use serde_json::json;
use trust_kernel::mcp::transport::{
    IncomingMessage, JsonRpcError, JsonRpcErrorBody, JsonRpcErrorCode, JsonRpcId,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, parse_line, write_message,
};

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

#[test]
fn parse_line_handles_request() {
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Request(req) => {
            assert_eq!(req.method, "tools/list");
            assert_eq!(req.id, JsonRpcId::Number(1));
        }
        other => panic!("expected Request, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Notification(notif) => {
            assert_eq!(notif.method, "notifications/initialized");
        }
        other => panic!("expected Notification, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_response_with_result() {
    let line = r#"{"jsonrpc":"2.0","id":5,"result":{"tools":[]}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Response(resp) => {
            assert_eq!(resp.id, JsonRpcId::Number(5));
            assert_eq!(resp.result, json!({"tools": []}));
        }
        other => panic!("expected Response, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_error_response() {
    let line = r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32601,"message":"not found"}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Error(err) => {
            assert_eq!(err.id, JsonRpcId::Number(9));
            assert_eq!(err.error.code, -32601);
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn parse_line_rejects_empty_input() {
    let result = parse_line("");
    assert!(matches!(result, Ok(None)));
}

#[test]
fn parse_line_returns_err_on_invalid_json() {
    let result = parse_line("{not valid json");
    assert!(result.is_err());
}

#[test]
fn write_message_emits_single_line_with_newline() {
    let resp = JsonRpcResponse::new(JsonRpcId::Number(1), json!({"ok": true}));
    let mut buf = Vec::new();
    write_message(&mut buf, &resp).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(!s.contains('\n') || s.matches('\n').count() == 1);
    assert!(s.contains(r#""ok":true"#));
}
