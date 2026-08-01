//! Compensation executor — V1.1 §7.2.
//!
//! W3a: `auto_reverse_move` for move_files. Each compensation record carries a
//! JSON `reverse_payload` of {moves: [{from, to}, ...]} where:
//!   - "from" = original source path (where the file should go back to)
//!   - "to"   = current destination path (where the file is now)
//!
//! auto_reverse swaps these: move file from `to` back to `from`.
//! On any failure mid-reverse: roll back the already-reversed moves.
//!
//! W10 Plan 2: `ReverseFnRegistry` routes reverse operations by
//! `compensate_fn` name. `auto_reverse(kernel, rec)` dispatcher looks up
//! the registered function and calls it. New reverse functions are
//! registered in `ReverseFnRegistry::new()`.
//!
//! ReverseFn signature is `fn(&TrustKernel, &CompensationRecord) -> Result<()>`
//! to support kernel access (reverse_form_prepare calls Playwright MCP via
//! `invoke_mcp_tool`). `auto_reverse_move` accepts `&TrustKernel` for
//! signature compatibility but does not use it (fs::rename needs no kernel).

use crate::compensation::types::CompensationRecord;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use std::collections::HashMap;
use std::path::PathBuf;

/// Type of reverse functions registered in `ReverseFnRegistry`.
///
/// W10 Plan 2: signature includes `&TrustKernel` so that reverse functions
/// needing MCP access (e.g. reverse_form_prepare → Playwright) can call
/// `invoke_mcp_tool(kernel, ...)`. Reverse functions that don't need kernel
/// (e.g. auto_reverse_move) accept the parameter with `_kernel` prefix.
pub type ReverseFn = fn(&TrustKernel, &CompensationRecord) -> Result<()>;

/// Registry mapping `compensate_fn` names to reverse function pointers.
///
/// W10 Plan 2: centralizes reverse function routing. New reverse functions
/// are registered in `new()`. The registry is constructed fresh on each
/// `auto_reverse` call (cheap: 4 HashMap inserts).
pub struct ReverseFnRegistry {
    fns: HashMap<String, ReverseFn>,
}

impl ReverseFnRegistry {
    /// Create a registry with all built-in reverse functions registered.
    /// W10 Plan 2 Task 7 will add 3 more entries (note/research/form).
    pub fn new() -> Self {
        let mut fns: HashMap<String, ReverseFn> = HashMap::new();
        fns.insert("filesystem.reverse_move".to_string(), auto_reverse_move);
        // W10 Plan 2 Task 7: 注册 note.reverse_capture / research.reverse_save / form.reverse_prepare
        Self { fns }
    }

    /// Look up and call the registered reverse function.
    /// Returns `Err(KernelError::Compensation(...))` if the name is not registered.
    pub fn call(&self, kernel: &TrustKernel, name: &str, rec: &CompensationRecord) -> Result<()> {
        let f = self
            .fns
            .get(name)
            .ok_or_else(|| KernelError::Compensation(format!("reverse fn not found: {}", name)))?;
        f(kernel, rec)
    }
}

/// Execute auto-reverse for a move_files compensation record.
/// Returns Ok(()) on success, Err on any failure (with rollback attempted).
/// Empty payload (no "moves" key, or empty array) is a no-op.
///
/// W10 Plan 2: signature extended with `&TrustKernel` for registry
/// compatibility. The kernel parameter is unused (fs::rename needs no
/// kernel access).
pub fn auto_reverse_move(_kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!("invalid reverse_payload: {}", e))
    })?;

    // Empty payload or missing/empty moves array → no-op success.
    let moves = match payload.get("moves").and_then(|m| m.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return Ok(()),
    };

    let mut reversed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for m in moves {
        let from = m
            .get("from")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Compensation("move entry missing 'from'".to_string()))?;
        let to = m
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Compensation("move entry missing 'to'".to_string()))?;

        let original = PathBuf::from(from);
        let current = PathBuf::from(to);

        if !current.exists() {
            // Roll back any already-reversed moves: undo rename(current -> original)
            // by renaming original back to current.
            for (orig, curr) in reversed.iter().rev() {
                let _ = std::fs::rename(orig, curr);
            }
            return Err(KernelError::Compensation(format!(
                "current location missing: {}",
                current.display()
            )));
        }

        // Ensure original parent dir exists (in case it was deleted).
        if let Some(parent) = original.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        match std::fs::rename(&current, &original) {
            Ok(_) => reversed.push((original.clone(), current.clone())),
            Err(e) => {
                // Roll back: undo rename(current -> original) by renaming original
                // back to current.
                for (orig, curr) in reversed.iter().rev() {
                    let _ = std::fs::rename(orig, curr);
                }
                return Err(KernelError::Compensation(format!(
                    "reverse move failed {} -> {}: {}",
                    current.display(),
                    original.display(),
                    e
                )));
            }
        }
    }

    Ok(())
}

/// W10 Plan 2: Dispatch reverse operation via `ReverseFnRegistry`.
///
/// Looks up `rec.compensate_fn` in the registry and calls the registered
/// function. This makes the `compensate_fn` field effective — previously
/// `auto_reverse_move` was hardcoded in task_compensate.rs.
///
/// **Stronghold note:** Caller (task_compensate.rs) is responsible for
/// decrypting `rec.reverse_payload` via `decrypt_compensation_if_needed`
/// before calling this function. The dispatcher and registered reverse
/// functions assume `reverse_payload` is already plaintext.
pub fn auto_reverse(kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    let registry = ReverseFnRegistry::new();
    registry.call(kernel, &rec.compensate_fn, rec)
}

#[cfg(test)]
mod w10_plan2_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;

    fn make_rec(compensate_fn: &str, payload: &str) -> CompensationRecord {
        CompensationRecord {
            comp_id: format!("comp-{}", uuid::Uuid::new_v4()),
            step_id: "s1".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: "2030-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: compensate_fn.to_string(),
            reverse_payload: payload.to_string(),
        }
    }

    #[test]
    fn reverse_fn_registry_routes_filesystem_reverse_move() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let registry = ReverseFnRegistry::new();
        let rec = make_rec("filesystem.reverse_move", r#"{"moves":[]}"#);
        let result = registry.call(&kernel, "filesystem.reverse_move", &rec);
        assert!(result.is_ok(), "expected Ok for filesystem.reverse_move, got {:?}", result.err());
    }

    #[test]
    fn reverse_fn_registry_unknown_fn_returns_err() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let registry = ReverseFnRegistry::new();
        let rec = make_rec("nonexistent.reverse", "{}");
        let result = registry.call(&kernel, "nonexistent.reverse", &rec);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("reverse fn not found"),
            "expected 'reverse fn not found' in error, got: {}",
            err
        );
    }

    #[test]
    fn auto_reverse_dispatches_via_compensate_fn_field() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec("filesystem.reverse_move", r#"{"moves":[]}"#);
        let result = auto_reverse(&kernel, &rec);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }
}
