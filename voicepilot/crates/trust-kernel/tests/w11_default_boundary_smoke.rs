//! W11 Plan 6 — V1 发布门禁 5 项 Fitness Functions(spec §9.2-§9.4)。
//!
//! 每个 Fitness Function 强制一项评测集的完整性(数据集 + Rust smoke 测试数量),
//! 与各 Plan 的真实 smoke 测试互补 —— 这里保证"评测集本身"不被意外删减:
//!
//!   1. `functional_coverage_100_tasks`  — 100_tasks.yaml 含 100 任务(50 单步 + 50 多步)+ canary
//!   2. `redteam_coverage_50_attacks`    — 50_attacks.yaml 含 50 攻击(5 类 × 10)+ canary
//!   3. `toctou_block_all_20`            — 20_scenarios.yaml 含 20 场景 + w11_toctou_block_smoke.rs 含 20 测试
//!   4. `malicious_server_block_all_15`  — 15_scenarios.yaml 含 15 场景 + w11_malicious_server_smoke.rs 含 15 测试
//!   5. `data_security_block_all_20`     — 20_scenarios.yaml 含 20 场景 + w11_data_security_smoke.rs 含 20 测试
//!
//! **路径定位:** CARGO_MANIFEST_DIR = voicepilot/crates/trust-kernel,
//! 上溯 3 级 = d:\voicepilot(仓库根),evals/ 在其下。default-gated(serde_yaml
//! 是 trust-kernel 非 optional 依赖,所有 feature 组合可用)。
//!
//! 运行:
//!   cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w11_default_boundary_smoke

use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

const CANARY: &str = "VP-EVAL-CANARY-2026";

/// 仓库根 = CARGO_MANIFEST_DIR 上溯 3 级。
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../..").canonicalize().unwrap_or(manifest.join("../../.."))
}

fn evals_dir() -> PathBuf {
    repo_root().join("evals")
}

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn read_file(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 统计 smoke 测试文件中 `#[test]` 测试函数数(按行精确匹配,避免注释干扰)。
fn count_tests(file: &str) -> usize {
    file.lines().filter(|l| l.trim() == "#[test]").count()
}

fn load_yaml(path: &PathBuf) -> Value {
    let content = read_file(path);
    serde_yaml::from_str(&content).unwrap_or_else(|e| {
        panic!("failed to parse YAML {}: {}", path.display(), e)
    })
}

fn seq_len(v: &Value) -> usize {
    v.as_sequence().map(|s| s.len()).unwrap_or(0)
}

/// Fitness Function 1:100 功能任务(①)。单步 50 + 多步 50 + canary。
#[test]
fn functional_coverage_100_tasks() {
    let yaml_path = evals_dir().join("functional/100_tasks.yaml");
    let content = read_file(&yaml_path);
    assert!(
        content.contains(CANARY),
        "100_tasks.yaml must contain canary {}",
        CANARY
    );
    let data = load_yaml(&yaml_path);
    let tasks = data
        .get("tasks")
        .expect("100_tasks.yaml must have 'tasks' key");
    assert_eq!(seq_len(tasks), 100, "must have exactly 100 tasks");

    let mut singles = 0;
    let mut multis = 0;
    for t in tasks.as_sequence().unwrap() {
        match t.get("type").and_then(|v| v.as_str()) {
            Some("single_step") => singles += 1,
            Some("multi_step") => multis += 1,
            other => panic!("unexpected task type: {:?}", other),
        }
    }
    assert_eq!(singles, 50, "must have exactly 50 single_step tasks");
    assert_eq!(multis, 50, "must have exactly 50 multi_step tasks");

    // 门禁基数:单步 ≥ 95%(≥ 48/50)+ 多步 ≥ 80%(≥ 40/50)
    assert!(singles >= 48, "single-step gate: need >= 48, got {}", singles);
    assert!(multis >= 40, "multi-step gate: need >= 40, got {}", multis);
}

/// Fitness Function 2:50 攻击样本(②)。5 类 × 10 + canary。
#[test]
fn redteam_coverage_50_attacks() {
    let yaml_path = evals_dir().join("redteam/50_attacks.yaml");
    let content = read_file(&yaml_path);
    assert!(content.contains(CANARY), "50_attacks.yaml must contain canary");
    let data = load_yaml(&yaml_path);
    let attacks = data
        .get("attacks")
        .expect("50_attacks.yaml must have 'attacks' key");
    assert_eq!(seq_len(attacks), 50, "must have exactly 50 attacks");

    use std::collections::HashMap;
    let mut per_category: HashMap<&str, usize> = HashMap::new();
    for a in attacks.as_sequence().unwrap() {
        let cat = a.get("category").and_then(|v| v.as_str()).expect("attack must have category");
        *per_category.entry(cat).or_insert(0) += 1;
    }
    for expected in ["prompt_extraction", "jailbreak", "pii", "excessive_agency", "hijacking"] {
        assert_eq!(
            per_category.get(expected).copied().unwrap_or(0),
            10,
            "category {} must have 10 attacks, got {:?}",
            expected,
            per_category
        );
    }
}

/// Fitness Function 3:20 TOCTOU 场景(③)。数据集 20 + smoke 测试 20。
#[test]
fn toctou_block_all_20() {
    let yaml_path = evals_dir().join("toctou/20_scenarios.yaml");
    let data = load_yaml(&yaml_path);
    let scenarios = data
        .get("scenarios")
        .expect("20_scenarios.yaml must have 'scenarios' key");
    assert_eq!(seq_len(scenarios), 20, "must have exactly 20 TOCTOU scenarios");

    let smoke = read_file(&tests_dir().join("w11_toctou_block_smoke.rs"));
    let test_count = count_tests(&smoke);
    assert_eq!(
        test_count, 20,
        "w11_toctou_block_smoke.rs must contain exactly 20 #[test] functions"
    );
}

/// Fitness Function 4:15 恶意 Server 场景(④)。数据集 15 + smoke 测试 15。
#[test]
fn malicious_server_block_all_15() {
    let yaml_path = evals_dir().join("malicious_server/15_scenarios.yaml");
    let data = load_yaml(&yaml_path);
    let scenarios = data
        .get("scenarios")
        .expect("15_scenarios.yaml must have 'scenarios' key");
    assert_eq!(seq_len(scenarios), 15, "must have exactly 15 malicious-server scenarios");

    let smoke = read_file(&tests_dir().join("w11_malicious_server_smoke.rs"));
    let test_count = count_tests(&smoke);
    assert_eq!(
        test_count, 15,
        "w11_malicious_server_smoke.rs must contain exactly 15 #[test] functions"
    );
}

/// Fitness Function 5:20 数据安全场景(⑤)。数据集 20 + smoke 测试 20。
#[test]
fn data_security_block_all_20() {
    let yaml_path = evals_dir().join("data_security/20_scenarios.yaml");
    let data = load_yaml(&yaml_path);
    let scenarios = data
        .get("scenarios")
        .expect("20_scenarios.yaml must have 'scenarios' key");
    assert_eq!(seq_len(scenarios), 20, "must have exactly 20 data-security scenarios");

    let smoke = read_file(&tests_dir().join("w11_data_security_smoke.rs"));
    let test_count = count_tests(&smoke);
    assert_eq!(
        test_count, 20,
        "w11_data_security_smoke.rs must contain exactly 20 #[test] functions"
    );
}
