//! Task 3 —— HTTP/SSE 网关契约测试（契约 §3.4）。
//!
//! 用 `tower::ServiceExt::oneshot` 直接驱动 Router，不需要真实端口；SSE 只读
//! **第一帧**（`body.frame()`），不 collect 整条流（否则心跳会让测试挂住）。
//!
//! 说明：`cli` 是 bin-only crate（没有 lib target），集成测试无法 `use cli::...`。
//! 这里用 `#[path]` 把 `src/serve.rs` 的**同一份源码**编进测试 crate，因此测的是
//! 真实 router / handler 实现，不是复制品。

#[path = "../src/serve.rs"]
// 测试 crate 只驱动 router，不调用 `run`（启动监听），所以整个模块的 dead_code
// 噪声一律关掉（不影响 bin 侧的 lint）。
#[allow(dead_code)]
mod serve;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::transaction::EffectManifest;

/// 建一个内存 DB 的 router（+ 状态，便于测试里直接注入 SSE 事件 / prepare token）。
fn test_app() -> (Router, serve::AppState) {
    let kernel = TrustKernel::open_in_memory().expect("open in-memory kernel");
    let state = serve::AppState::new(kernel);
    (serve::router(state.clone()), state)
}

/// POST 一个 JSON body，返回 (status, JSON)。
async fn post_json(app: &Router, uri: &str, body: &Value) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("router call");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "{uri} returned non-JSON body ({e}): {:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, value)
}

/// GET，返回 (status, headers, body bytes)。
async fn get_raw(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router call");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    (status, headers, bytes.to_vec())
}

/// 契约 §3.4①：快路由 → 200 + kind=skill + route_kind=fastroute。
#[tokio::test]
async fn route_fast_route_returns_skill_with_fastroute_kind() {
    let (app, _state) = test_app();
    let (status, body) = post_json(
        &app,
        "/api/v1/route",
        &json!({"text": "打开 https://example.com/a"}),
    )
    .await;

    println!("/api/v1/route → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "skill");
    assert_eq!(body["skill_id"], "sys.open_url");
    assert_eq!(body["route_kind"], "fastroute");
    assert_eq!(body["used_llm"], false);
    assert_eq!(body["slots"][0]["kind"], "url");
    assert_eq!(body["slots"][0]["raw"], "https://example.com/a");
    assert!(body["latency_ms"].as_i64().unwrap() >= 0);
    assert!(body["dag"].is_null());
    assert!(body["text"].is_null());
}

/// 空输入 → kind=empty（契约 §3.2 映射表）。
#[tokio::test]
async fn route_empty_text_returns_empty_kind() {
    let (app, _state) = test_app();
    let (status, body) = post_json(&app, "/api/v1/route", &json!({"text": "   "})).await;
    println!("/api/v1/route (empty) → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "empty");
    assert!(body["skill_id"].is_null());
}

/// 契约 §3.4②：重复 `idempotency_key` → 第二次 `idempotent_replay=true`，不二次执行。
#[tokio::test]
async fn execute_replays_same_idempotency_key_without_reexecuting() {
    let (app, _state) = test_app();
    let body = json!({
        "task_id": "task-serve-idem",
        "step_id": "step-serve-idem",
        "skill_id": "sys.datetime",
        "slots": {},
        "idempotency_key": "idem-key-1",
    });

    let (first_status, first) = post_json(&app, "/api/v1/execute", &body).await;
    println!("/api/v1/execute (1st) → {first_status} {first}");
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(first["committed"], true);
    assert_eq!(first["idempotent_replay"], false);
    assert!(first["error"].is_null());
    let first_summary = first["summary"].as_str().unwrap().to_string();
    assert!(
        !first_summary.is_empty(),
        "executor should return a summary"
    );

    let (second_status, second) = post_json(&app, "/api/v1/execute", &body).await;
    println!("/api/v1/execute (2nd) → {second_status} {second}");
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(second["idempotent_replay"], true);
    assert_eq!(second["summary"], first["summary"]);
    assert_eq!(second["committed"], first["committed"]);
}

/// 契约 §3.4③：伪造 / 篡改 manifest 的 prepare_token → 409 且不执行。
#[tokio::test]
async fn execute_rejects_forged_and_tampered_prepare_tokens() {
    let (app, state) = test_app();
    let manifest = json!({
        "sources": [],
        "destination": "C:/dest-a",
        "conflicts": [],
        "total_bytes": 0,
    });
    let base = |token: &str, manifest: Value| {
        json!({
            "task_id": "task-serve-token",
            "step_id": "step-serve-token",
            "skill_id": "sys.datetime",
            "slots": {},
            "idempotency_key": format!("idem-{token}"),
            "prepare_token": token,
            "manifest": manifest,
        })
    };

    // ① 伪造 token
    let (status, body) = post_json(
        &app,
        "/api/v1/execute",
        &base("prt_forged_token", manifest.clone()),
    )
    .await;
    println!("/api/v1/execute (forged token) → {status} {body}");
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("prepare token"));

    // ② 真实 token + 被篡改的 manifest
    let prepared = EffectManifest {
        sources: vec![],
        destination: "C:/dest-a".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let token = state
        .kernel
        .transaction_manager()
        .prepare("task-serve-token", "step-serve-token", &prepared)
        .expect("prepare token");
    let tampered = json!({
        "sources": [],
        "destination": "C:/dest-b",
        "conflicts": [],
        "total_bytes": 0,
    });
    let (status, body) = post_json(&app, "/api/v1/execute", &base(&token.token, tampered)).await;
    println!("/api/v1/execute (tampered manifest) → {status} {body}");
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("precondition mismatch")
    );

    // ③ 同一个 token + 原 manifest → 放行（validate 只读，不消费 token）
    let (status, body) = post_json(
        &app,
        "/api/v1/execute",
        &base(&token.token, manifest.clone()),
    )
    .await;
    println!("/api/v1/execute (valid token) → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["committed"], true);
}

/// prepare_token 缺 manifest → 409（契约 §3.3：token 必须伴随 manifest）。
#[tokio::test]
async fn execute_rejects_prepare_token_without_manifest() {
    let (app, _state) = test_app();
    let (status, body) = post_json(
        &app,
        "/api/v1/execute",
        &json!({
            "task_id": "t", "step_id": "s", "skill_id": "sys.datetime",
            "slots": {}, "idempotency_key": "idem-no-manifest",
            "prepare_token": "prt_x",
        }),
    )
    .await;
    println!("/api/v1/execute (token without manifest) → {status} {body}");
    assert_eq!(status, StatusCode::CONFLICT);
}

/// 契约 §3.4④：`/events` → content-type `text/event-stream`，注入事件后收到 `data:` 帧。
#[tokio::test]
async fn events_streams_sse_data_frames() {
    let (app, state) = test_app();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/events")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router call");

    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("content-type")
        .to_str()
        .unwrap()
        .to_string();
    println!("/api/v1/events content-type = {content_type}");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content-type: {content_type}"
    );

    // 订阅已建立（handler 已在 oneshot 内 subscribe），此时注入事件。
    let mut body = res.into_body();
    state
        .events
        .send(serve::ServerEvent::new(
            "audit",
            Some("task-serve-sse"),
            json!({"hello": "world"}),
        ))
        .expect("broadcast send");

    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("timed out waiting for the first SSE frame")
        .expect("stream ended without a frame")
        .expect("frame error");
    let data = frame.into_data().expect("first frame must carry data");
    let text = String::from_utf8(data.to_vec()).expect("utf8 frame");
    println!("/api/v1/events first frame = {text:?}");
    assert!(text.starts_with("data: "), "frame = {text:?}");
    assert!(
        text.contains("\"event_type\":\"audit\""),
        "frame = {text:?}"
    );
    assert!(text.contains("\"hello\":\"world\""), "frame = {text:?}");
    assert!(
        text.ends_with("\n\n"),
        "SSE frame must end with a blank line"
    );
}

/// 契约 §3.4⑤：`/observability` → 200 且 `audit_chain.valid=true`，routes 含采样。
#[tokio::test]
async fn observability_reports_routes_and_valid_audit_chain() {
    let (app, _state) = test_app();
    // 先打一次 /route 产生一条 route 样本。
    let (route_status, route_body) = post_json(
        &app,
        "/api/v1/route",
        &json!({"text": "打开 https://example.com/a"}),
    )
    .await;
    assert_eq!(route_status, StatusCode::OK, "route: {route_body}");

    let (status, headers, bytes) = get_raw(&app, "/api/v1/observability?window_hours=24").await;
    let body: Value = serde_json::from_slice(&bytes).expect("observability JSON");
    println!("/api/v1/observability → {status} {body}");

    assert_eq!(status, StatusCode::OK);
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(body["window_hours"], 24);
    assert_eq!(body["audit_chain"]["valid"], true);
    assert_eq!(body["audit_chain"]["checked"], 0);
    assert!(body["audit_chain"]["broken_at"].is_null());

    // routes：刚才的 /route 走 fastroute，必然有对应分桶。
    let routes = body["routes"].as_array().expect("routes array");
    let fastroute = routes
        .iter()
        .find(|r| r["route_kind"] == "fastroute")
        .expect("fastroute bucket from the /route sample");
    assert_eq!(fastroute["sample_count"], 1);

    // 窗口内没有 cache / llm 样本 → 命中率 0.0，DAG token 全 0。
    assert_eq!(body["cache"]["hits"], 0);
    assert_eq!(body["cache"]["misses"], 0);
    assert_eq!(body["cache"]["hit_rate"], 0.0);
    assert_eq!(body["dag_tokens"]["plans"], 0);
    assert_eq!(body["dag_tokens"]["total_tokens"], 0);
}

/// 不带 `window_hours` → 默认 24 小时（契约 §3.2 可选参数）。
#[tokio::test]
async fn observability_defaults_to_24_hours() {
    let (app, _state) = test_app();
    let (status, _headers, bytes) = get_raw(&app, "/api/v1/observability").await;
    let body: Value = serde_json::from_slice(&bytes).expect("observability JSON");
    println!("/api/v1/observability (default window) → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["window_hours"], 24);
    assert_eq!(body["routes"].as_array().unwrap().len(), 0);
}

/// `/compensate` 无 active 补偿记录 → 200 且 compensated=false（不 panic、不 500）。
#[tokio::test]
async fn compensate_without_active_record_reports_failure() {
    let (app, _state) = test_app();
    let (status, body) = post_json(
        &app,
        "/api/v1/compensate",
        &json!({
            "task_id": "task-serve-comp",
            "step_id": "step-serve-comp",
            "target_step_id": "step-never-existed",
        }),
    )
    .await;
    println!("/api/v1/compensate → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compensated"], false);
    assert!(body["error"].is_string());
}

/// 契约 §3.4③：**过期** prepare_token → 409（评审 Important #1 补的用例）。
///
/// 用 `prepare_with_ttl(-1)` 造一个已过期 token，走真实 HTTP 层，
/// 证明 `TransactionManager::validate` 的过期分支确实在网关里生效。
#[tokio::test]
async fn execute_rejects_expired_prepare_token() {
    let (app, state) = test_app();
    let manifest = EffectManifest {
        sources: vec![],
        destination: "C:/dest-expired".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let token = state
        .kernel
        .transaction_manager()
        .prepare_with_ttl("task-serve-exp", "step-serve-exp", &manifest, -1)
        .expect("prepare expired token");

    let (status, body) = post_json(
        &app,
        "/api/v1/execute",
        &json!({
            "task_id": "task-serve-exp",
            "step_id": "step-serve-exp",
            "skill_id": "sys.datetime",
            "slots": {},
            "idempotency_key": "idem-expired-token",
            "prepare_token": token.token,
            "manifest": {
                "sources": [],
                "destination": "C:/dest-expired",
                "conflicts": [],
                "total_bytes": 0,
            },
        }),
    )
    .await;
    println!("/api/v1/execute (expired token) → {status} {body}");
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"].as_str().unwrap().contains("expired"),
        "expected expiry rejection, got: {body}"
    );
}

/// 安全边界：需要人工审批的技能经 HTTP 一律 fail-closed（评审 Important #2）。
///
/// `approval_required` 白名单（fs.write_file / fs.create_file / fs.delete_file /
/// shell.run）在网关侧没有审批者，必须直接拒绝执行，而不是用 AutoApprover 放行。
#[tokio::test]
async fn execute_rejects_approval_required_skills_fail_closed() {
    let (app, _state) = test_app();
    let (status, body) = post_json(
        &app,
        "/api/v1/execute",
        &json!({
            "task_id": "task-serve-approval",
            "step_id": "step-serve-approval",
            "skill_id": "fs.delete_file",
            "slots": {"path": "C:/tmp/whatever.txt"},
            "idempotency_key": "idem-approval-required",
        }),
    )
    .await;
    println!("/api/v1/execute (approval-required skill) → {status} {body}");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["committed"], false);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("requires interactive approval"),
        "expected fail-closed approval rejection, got: {body}"
    );
}
