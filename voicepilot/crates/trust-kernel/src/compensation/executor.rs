//! Compensation executor — V1.1 §7.2.
//!
//! W3a: `auto_reverse_move` for move_files. Each compensation record carries a
//! JSON `reverse_payload` of {moves: [{from, to}, ...]} where:
//!   - "from" = original source path (where the file should go back to)
//!   - "to"   = current destination path (where the file is now)
//!
//! auto_reverse swaps these: move file from `to` back to `from`.
//! On any failure mid-reverse: roll back the already-reversed moves.

use crate::compensation::types::CompensationRecord;
use crate::error::{KernelError, Result};
use std::path::PathBuf;

/// Execute auto-reverse for a move_files compensation record.
/// Returns Ok(()) on success, Err on any failure (with rollback attempted).
/// Empty payload (no "moves" key, or empty array) is a no-op.
pub fn auto_reverse_move(rec: &CompensationRecord) -> Result<()> {
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
