//! W7 Plan 3 Task 7 — end-to-end smoke tests for user-custom Skill loading.
//!
//! Test 1 (`scan_loads_valid_skill_into_router`): tempdir + 写一个合法
//! `my-test.md` → `scan_user_skills` 返回 1 个 manifest → 注册到
//! `SkillRouter` 后 `route` 命中(keyword 匹配)。
//!
//! Test 2 (`scan_skips_malformed_yaml`): tempdir + 写一个 `bad.md`
//! (无效 YAML)→ `scan_user_skills` 返回 0 个 manifest。
//!
//! Test 3 (`user_skill_overrides_built_in_same_id`): 先注册 built-in
//! `files_organize_manifest()`,再注册同 id `files.organize` 的用户版本
//! (title 不同)→ `route("整理下载目录")` 返回用户版本。

use std::fs;
use std::path::PathBuf;

use trust_kernel::skills::manifest::files_organize_manifest;
use trust_kernel::skills::router::{RouteDecision, SkillRouter};
use trust_kernel::skills::user_loader::{parse_skill_md, scan_user_skills};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "voicepilot-w7p3-smoke-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 构造一个合法的 Skill .md 内容,id/title/keywords 可定制。
fn build_md(id: &str, title: &str, keyword: &str) -> String {
    format!(
        "---\n\
id: {id}\n\
version: \"1.0.0\"\n\
title: {title}\n\
description: smoke-test skill\n\
intent_examples:\n  - 部署项目到生产环境\n\
keywords:\n  - {keyword}\n\
inputs: {{}}\n\
risk_ceiling: E2\n\
data_class_ceiling: D2\n\
egress: local_only\n\
max_steps: 3\n\
tools: []\n\
approval:\n  mode: none\n  required_for: commit\n  show_effect_manifest: false\n  max_approval_scope: 0\n\
compensation:\n  level: none\n  ttl_seconds: 0\n  conflict_policy: auto_reverse\n\
verifier:\n  strategy: weak\n  recheck_after_seconds: 0\n\
failure_policy:\n  max_retries: 0\n  allow_replan: false\n  on_fail: stop\n\
---\n\n# {title}\n\n这是一个 smoke-test 用途的用户自定义 Skill body。\n",
        id = id,
        title = title,
        keyword = keyword,
    )
}

#[test]
fn scan_loads_valid_skill_into_router() {
    let dir = tmp_dir();
    fs::write(dir.join("my-test.md"), build_md("my.test", "My Test", "部署")).unwrap();
    // 干扰文件:非 .md 应被忽略。
    fs::write(dir.join("notes.txt"), "ignore me").unwrap();

    let manifests = scan_user_skills(&dir);
    assert_eq!(manifests.len(), 1, "expected exactly 1 valid manifest");
    assert_eq!(manifests[0].id, "my.test");
    assert_eq!(manifests[0].title, "My Test");
    // description_body 应被填充(包含 "# My Test" heading)。
    assert!(
        manifests[0].description_body.as_deref().unwrap().contains("# My Test"),
        "description_body must contain markdown body"
    );

    // 注册到 SkillRouter 后 route 命中。
    let mut router = SkillRouter::new();
    router.register(manifests.into_iter().next().unwrap());
    let decision = router.route("部署项目");
    match decision {
        RouteDecision::Skill(m) => {
            assert_eq!(m.id, "my.test");
            assert_eq!(m.title, "My Test");
        }
        other => panic!("expected Skill decision, got {:?}", other),
    }

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_skips_malformed_yaml() {
    let dir = tmp_dir();
    // bad.md: frontmatter 缺关键字段 + 未闭合的 YAML 序列。
    let bad_md = "---\nid: bad.skill\nbad: [unclosed\n---\nbody\n";
    fs::write(dir.join("bad.md"), bad_md).unwrap();

    let manifests = scan_user_skills(&dir);
    assert!(
        manifests.is_empty(),
        "malformed YAML must be skipped, got {} manifests",
        manifests.len()
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn user_skill_overrides_built_in_same_id() {
    // 1. 注册 built-in files.organize(title = "整理文件")。
    let mut router = SkillRouter::new();
    let built_in = files_organize_manifest();
    assert_eq!(built_in.id, "files.organize");
    assert_eq!(built_in.title, "整理文件");
    router.register(built_in);

    // 2. 构造一个同 id 的用户版本,但 title 不同。
    //    使用 parse_skill_md 从字符串解析,确保真实走 user_loader 路径。
    let user_md = build_md("files.organize", "User Override Title", "整理");
    let (user_manifest, _) = parse_skill_md(&user_md).expect("user md must parse");
    assert_eq!(user_manifest.id, "files.organize");
    assert_eq!(user_manifest.title, "User Override Title");

    // 3. 注册用户版本(同 id,应覆盖 built-in)。
    router.register(user_manifest);

    // 4. route 命中后应返回用户版本(title = "User Override Title")。
    let decision = router.route("整理下载目录");
    match decision {
        RouteDecision::Skill(m) => {
            assert_eq!(m.id, "files.organize");
            assert_eq!(
                m.title, "User Override Title",
                "user manifest must override built-in"
            );
        }
        other => panic!("expected Skill decision, got {:?}", other),
    }
}

// ---- W7 Plan 6 Task 3: route_with_llm strengthening ----

/// W7 Plan 6 Task 3 Step 1: a user-custom Skill must appear in the
/// candidate list sent to the LLM by `route_with_llm`. Proves this by
/// mounting a wiremock that returns `matched_skill_id = "my.test"` —
/// if the user manifest weren't registered, the LLM's response would
/// fall through to Planner (skill id not found in `self.skills`).
#[cfg(feature = "llm")]
#[tokio::test]
async fn user_skill_appears_as_llm_candidate_via_route_with_llm() {
    use std::sync::Arc;
    use trust_kernel::llm::client::LlmClient;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let dir = tmp_dir();
    fs::write(dir.join("my-test.md"), build_md("my.test", "My Test Skill", "test")).unwrap();

    let manifests = scan_user_skills(&dir);
    assert_eq!(manifests.len(), 1, "expected exactly 1 valid manifest");
    let user_manifest = manifests.into_iter().next().unwrap();
    assert_eq!(user_manifest.id, "my.test");
    assert_eq!(user_manifest.title, "My Test Skill");

    // Mount wiremock returning the user skill id with high confidence.
    // If the user manifest is in the candidate list, the router will
    // find it by id and return RouteDecision::Skill.
    let server = MockServer::start().await;
    let arguments = serde_json::json!({
        "matched_skill_id": "my.test",
        "confidence": 0.9,
        "slots": [],
        "reasoning": "user wants to run my.test"
    })
    .to_string();
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "route_skill",
                        "arguments": arguments,
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test-model"));
    let mut router = SkillRouter::with_llm(llm);
    router.register(user_manifest);

    // Input "请执行操作" contains no substring "test" (the keyword) and
    // no intent_example ("部署项目到生产环境") — falls through to LLM.
    let decision = router.route_with_llm("请执行操作").await;
    match decision {
        RouteDecision::Skill(m) => {
            assert_eq!(m.id, "my.test");
            assert_eq!(m.title, "My Test Skill");
        }
        other => panic!("expected Skill decision, got {:?}", other),
    }

    fs::remove_dir_all(&dir).ok();
}

/// W7 Plan 6 Task 3 Step 2: when a user manifest with the same id as a
/// built-in is registered AFTER the built-in (override), and the LLM
/// returns that shared id, the router must return the USER version
/// (not the built-in). Proves the override also holds for the LLM
/// fallback path, not just the keyword path covered by
/// `user_skill_overrides_built_in_same_id`.
#[cfg(feature = "llm")]
#[tokio::test]
async fn user_skill_overrides_built_in_when_llm_returns_same_id() {
    use std::sync::Arc;
    use trust_kernel::llm::client::LlmClient;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let dir = tmp_dir();
    fs::write(
        dir.join("files-organize.md"),
        build_md("files.organize", "User Custom Organize", "整理"),
    )
    .unwrap();

    let manifests = scan_user_skills(&dir);
    assert_eq!(manifests.len(), 1, "expected exactly 1 valid manifest");
    let user_manifest = manifests.into_iter().next().unwrap();
    assert_eq!(user_manifest.id, "files.organize");
    assert_eq!(user_manifest.title, "User Custom Organize");

    // Mount wiremock returning the shared id with high confidence.
    let server = MockServer::start().await;
    let arguments = serde_json::json!({
        "matched_skill_id": "files.organize",
        "confidence": 0.9,
        "slots": [],
        "reasoning": "user wants to organize files"
    })
    .to_string();
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "route_skill",
                        "arguments": arguments,
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test-model"));
    let mut router = SkillRouter::with_llm(llm);
    // Register built-in FIRST, then user version (same id, must override).
    router.register(files_organize_manifest());
    router.register(user_manifest);

    // Input "perform the operation" contains no keyword from either the
    // built-in (整理/归档/移动文件/下载目录) nor the user manifest (整理),
    // and no intent_example substring — falls through to LLM.
    let decision = router.route_with_llm("perform the operation").await;
    match decision {
        RouteDecision::Skill(m) => {
            assert_eq!(m.id, "files.organize");
            assert_eq!(
                m.title, "User Custom Organize",
                "user manifest must override built-in when LLM returns same id"
            );
        }
        other => panic!("expected Skill decision, got {:?}", other),
    }

    fs::remove_dir_all(&dir).ok();
}
