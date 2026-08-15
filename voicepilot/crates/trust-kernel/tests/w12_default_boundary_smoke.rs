//! W12 Plan 6 — V1 工程规范 5 项 Fitness Functions(spec §九 Plan 6)。
//!
//! 每个 Fitness Function 强制一项工程规范配置的完整性,防止配置被意外删减:
//!
//!   1. `deny_toml_exists`        — deny.toml 存在 + 含 [licenses]/[advisories]/[bans]/[sources] 4 section
//!   2. `ci_workflow_exists`      — .github/workflows/ci.yml 存在 + 含 5 个 job
//!   3. `adr_completeness`        — docs/adr/ 含 ≥ 11 个 ADR 文件
//!   4. `cargo_edition_2024`      — workspace Cargo.toml edition = "2024"
//!   5. `nsis_bundle_configured`  — tauri.conf.json bundle.targets 含 "nsis"
//!
//! **路径定位:** CARGO_MANIFEST_DIR = voicepilot/crates/trust-kernel,
//! 上溯 3 级 = d:\voicepilot(仓库根)。default-gated(仅 std + serde_json/fs,
//! trust-kernel 所有 feature 组合可用)。
//!
//! 运行:
//!   cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w12_default_boundary_smoke

use std::fs;
use std::path::PathBuf;

/// 仓库根 = CARGO_MANIFEST_DIR 上溯 3 级。
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../..").canonicalize().unwrap_or(manifest.join("../../.."))
}

fn read_file(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// Fitness Function 1:deny.toml 存在 + 含 4 个关键 section。
#[test]
fn deny_toml_exists() {
    let deny = read_file(&repo_root().join("deny.toml"));
    for section in ["[licenses]", "[advisories]", "[bans]", "[sources]"] {
        assert!(
            deny.contains(section),
            "deny.toml must contain section {}",
            section
        );
    }
}

/// Fitness Function 2:ci.yml 存在 + 含 5 个 job(lint / test-default / test-full / build-ui / evals)。
#[test]
fn ci_workflow_exists() {
    let ci = read_file(&repo_root().join(".github/workflows/ci.yml"));
    for job in ["lint", "test-default", "test-full", "build-ui", "evals"] {
        assert!(
            // 匹配 "  <job>:" 作为 YAML job 定义(顶格缩进 2 空格 + 名称 + 冒号)
            ci.contains(&format!("  {}:", job)),
            "ci.yml must define job {}",
            job
        );
    }
}

/// Fitness Function 3:docs/adr/ 含 ≥ 11 个 ADR 文件(0001-0011,不含 template)。
#[test]
fn adr_completeness() {
    let adr_dir = repo_root().join("docs/adr");
    let entries = fs::read_dir(&adr_dir)
        .unwrap_or_else(|e| panic!("failed to read docs/adr: {}", e));
    let adr_count = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("0")
                && e.file_name().to_string_lossy().ends_with(".md")
        })
        .count();
    assert!(
        adr_count >= 11,
        "docs/adr/ must contain >= 11 ADR files, got {}",
        adr_count
    );
}

/// Fitness Function 4:workspace Cargo.toml edition = "2024"。
#[test]
fn cargo_edition_2024() {
    let cargo = read_file(&repo_root().join("voicepilot/Cargo.toml"));
    assert!(
        cargo.contains("edition = \"2024\""),
        "workspace Cargo.toml must set edition = 2024"
    );
}

/// Fitness Function 5:tauri.conf.json bundle.targets 含 "nsis"。
#[test]
fn nsis_bundle_configured() {
    let conf = read_file(&repo_root().join("voicepilot/crates/ui/tauri.conf.json"));
    assert!(
        conf.contains("\"nsis\""),
        "tauri.conf.json bundle.targets must contain nsis"
    );
}