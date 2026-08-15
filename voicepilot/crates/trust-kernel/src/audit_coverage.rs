//! W10 Plan 5 — 审计事件覆盖率检查器(spec §7.2)。
//!
//! `AuditCoverageChecker` 查询 `audit_logs` 表的 `event_type` 分布,与
//! `AUDIT_EVENT_TYPE_REGISTRY`(或自定义 expected 列表)对比,计算覆盖率。
//!
//! 用法:
//! ```ignore
//! use trust_kernel::audit_coverage::AuditCoverageChecker;
//! use trust_kernel::kernel::TrustKernel;
//!
//! let kernel = TrustKernel::open_in_memory().unwrap();
//! // 触发事件...
//! let checker = AuditCoverageChecker::new(&kernel);
//! let uncovered = checker.uncovered().unwrap();
//! let ratio = checker.coverage_ratio().unwrap();
//! println!("coverage: {:.1}% ({}/{})", ratio * 100.0, checker.covered().unwrap().len(), checker.expected.len());
//! ```
//!
//! **feature gate:** default(纯 DB 操作,不依赖 voice/stronghold feature)。
//! default feature 下,`voice_started` / `stronghold_snapshot_encrypted` /
//! `stronghold_snapshot_decrypt_failed` 3 种事件不可触发,需用
//! `with_expected` 传入 25 种 default-reachable 子集断言 100% 覆盖。

use crate::audit::AUDIT_EVENT_TYPE_REGISTRY;
use crate::error::Result;
use crate::kernel::TrustKernel;

/// W10 Plan 5: 审计事件覆盖率检查器(spec §7.2)。
///
/// 持有 `&TrustKernel` 引用 + `expected` 事件类型列表(默认用全量
/// `AUDIT_EVENT_TYPE_REGISTRY`)。`covered()` 查询 audit_logs 表的
/// DISTINCT event_type,与 expected 对比计算覆盖率。
pub struct AuditCoverageChecker<'a> {
    kernel: &'a TrustKernel,
    expected: &'a [&'a str],
}

impl<'a> AuditCoverageChecker<'a> {
    /// 用全量 registry(28 种)构造 checker。
    ///
    /// 适用于全 feature 验收测试(voice,tauri,llm,uia,stronghold 全开)。
    pub fn new(kernel: &'a TrustKernel) -> Self {
        Self {
            kernel,
            expected: AUDIT_EVENT_TYPE_REGISTRY,
        }
    }

    /// 用自定义 expected 列表构造 checker。
    ///
    /// 适用于 default feature 测试 —— 排除 voice_started / stronghold_*
    /// 等不可触发事件,断言 default-reachable 子集 100% 覆盖。
    pub fn with_expected(kernel: &'a TrustKernel, expected: &'a [&'a str]) -> Self {
        Self { kernel, expected }
    }

    /// 返回 expected 列表(便于 caller 获取分母)。
    pub fn expected(&self) -> &[&str] {
        self.expected
    }

    /// 查询 audit_logs 表,返回已覆盖的 event_type 列表(去重,字母序)。
    ///
    /// 仅返回 expected 中存在的事件(忽略 audit_logs 中的未知 event_type,
    /// 避免历史脏数据干扰覆盖率计算)。
    pub fn covered(&self) -> Result<Vec<String>> {
        let conn = self.kernel.conn();
        let mut stmt = conn.prepare("SELECT DISTINCT event_type FROM audit_logs")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut all: Vec<String> = Vec::new();
        for r in rows {
            all.push(r?);
        }
        drop(stmt);
        drop(conn);
        // 仅保留 expected 中的事件,字母序排序
        let mut covered: Vec<String> = all
            .into_iter()
            .filter(|e| self.expected.contains(&e.as_str()))
            .collect();
        covered.sort();
        covered.dedup();
        Ok(covered)
    }

    /// 返回 expected 中尚未被覆盖的 event_type 列表(字母序)。
    pub fn uncovered(&self) -> Result<Vec<String>> {
        let covered = self.covered()?;
        let mut uncovered: Vec<String> = self
            .expected
            .iter()
            .filter(|e| !covered.iter().any(|c| c == *e))
            .map(|s| s.to_string())
            .collect();
        uncovered.sort();
        Ok(uncovered)
    }

    /// 返回覆盖率 = covered.len() / expected.len()。
    ///
    /// 若 expected 为空,返回 1.0(避免除零;空 expected 视为 100% 满足)。
    pub fn coverage_ratio(&self) -> Result<f64> {
        if self.expected.is_empty() {
            return Ok(1.0);
        }
        let covered = self.covered()?;
        Ok(covered.len() as f64 / self.expected.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::TrustKernel;

    /// 辅助:创建 task 并 emit 一组事件,验证 coverage 计算。
    fn emit_events(kernel: &TrustKernel, task_id: &str, events: &[&str]) {
        use crate::repo::task_repo::{TaskRecord, TaskRepo};
        let conn = kernel.conn();
        let task = TaskRecord::new(task_id, "coverage test placeholder");
        TaskRepo::new().create(&conn, &task).unwrap();
        drop(conn);
        for et in events {
            kernel
                .audit_append_external(task_id, None, et, serde_json::json!({"test": true}))
                .unwrap();
        }
    }

    #[test]
    fn covered_returns_only_expected_events() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        emit_events(&kernel, "t1", &["task_created", "state_transition", "unknown_event"]);
        let checker = AuditCoverageChecker::new(&kernel);
        let covered = checker.covered().unwrap();
        // unknown_event 不在 registry,被过滤
        assert!(covered.contains(&"task_created".to_string()));
        assert!(covered.contains(&"state_transition".to_string()));
        assert!(!covered.iter().any(|c| c == "unknown_event"));
    }

    #[test]
    fn uncovered_returns_missing_events() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // 只 emit 一个事件,其余 28 种应出现在 uncovered
        emit_events(&kernel, "t1", &["task_created"]);
        let checker = AuditCoverageChecker::new(&kernel);
        let uncovered = checker.uncovered().unwrap();
        assert!(!uncovered.contains(&"task_created".to_string()));
        assert!(uncovered.contains(&"state_transition".to_string()));
        assert!(uncovered.contains(&"voice_started".to_string()));
        // 总 registry 29 种,覆盖 1 种,未覆盖 28 种
        assert_eq!(uncovered.len(), 28);
    }

    #[test]
    fn coverage_ratio_computes_correctly() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        emit_events(&kernel, "t1", &["task_created", "state_transition"]);
        let checker = AuditCoverageChecker::new(&kernel);
        let ratio = checker.coverage_ratio().unwrap();
        // 2/29 ≈ 0.0690
        assert!((ratio - 2.0 / 29.0).abs() < 1e-6, "ratio must be 2/29, got {}", ratio);
    }

    #[test]
    fn with_expected_uses_custom_subset() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        emit_events(&kernel, "t1", &["task_created", "state_transition"]);
        // 自定义 expected 仅 2 种,都已覆盖 → 100%
        let subset: &[&str] = &["task_created", "state_transition"];
        let checker = AuditCoverageChecker::with_expected(&kernel, subset);
        let uncovered = checker.uncovered().unwrap();
        assert!(uncovered.is_empty(), "uncovered must be empty for full subset coverage");
        let ratio = checker.coverage_ratio().unwrap();
        assert!((ratio - 1.0).abs() < 1e-6, "ratio must be 1.0 for full coverage");
    }

    #[test]
    fn empty_expected_returns_full_coverage() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let empty: &[&str] = &[];
        let checker = AuditCoverageChecker::with_expected(&kernel, empty);
        let ratio = checker.coverage_ratio().unwrap();
        assert!((ratio - 1.0).abs() < 1e-6, "empty expected must return 1.0");
    }
}
