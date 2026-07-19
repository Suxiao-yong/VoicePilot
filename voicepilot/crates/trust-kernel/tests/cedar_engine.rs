use trust_kernel::policy::cedar_engine::CedarEngine;
use trust_kernel::policy::types::{Action, DLevel, ELevel, Resource};

fn make_action(name: &str, e: ELevel) -> Action {
    Action { name: name.to_string(), e_level: e }
}

fn make_resource(path: &str, d: DLevel) -> Resource {
    Resource {
        path: path.to_string(),
        data_class: d,
        provenance: "user_direct".to_string(),
    }
}

fn load_default() -> CedarEngine {
    let src = include_str!("../src/policies/default.cedar");
    CedarEngine::from_source(src).expect("default cedar policy must parse")
}

#[test]
fn d0_public_doc_read_is_allowed() {
    let engine = load_default();
    let r = make_resource("/docs/readme.md", DLevel::D0);
    let a = make_action("read_file", ELevel::E0);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn d3_credential_read_is_denied() {
    let engine = load_default();
    let r = make_resource("/secrets/token.txt", DLevel::D3);
    let a = make_action("read_file", ELevel::E0);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn d2_private_doc_read_is_allowed_by_cedar_upgraded_to_confirm_by_constraint() {
    // Cedar permits D2 read; Constraint Engine upgrades effect to confirm.
    // Cedar itself returns true here — the upgrade happens in the gateway.
    let engine = load_default();
    let r = make_resource("/docs/private.md", DLevel::D2);
    let a = make_action("read_file", ELevel::E0);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn shell_exec_is_always_denied() {
    let engine = load_default();
    let r = make_resource("/bin/sh", DLevel::D0);
    let a = make_action("shell_exec", ELevel::E3);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn send_to_remote_llm_d3_denied() {
    let engine = load_default();
    let r = make_resource("/secrets/cookie.txt", DLevel::D3);
    let a = make_action("send_to_remote_llm", ELevel::E3);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn send_to_remote_llm_d0_allowed() {
    let engine = load_default();
    let r = make_resource("/docs/public.md", DLevel::D0);
    let a = make_action("send_to_remote_llm", ELevel::E3);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn malformed_cedar_source_returns_parse_error() {
    let result = CedarEngine::from_source("this is not cedar");
    assert!(result.is_err());
}
