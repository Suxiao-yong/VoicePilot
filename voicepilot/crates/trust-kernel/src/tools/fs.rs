//! FilesystemTool — V1.1 §6.1, §6.2.
//!
//! W3a scope:
//!   - prepare_move: snapshot sources → EffectManifest → prepare_token
//!   - commit_move: re-check preconditions, execute move, return ToolResult
//!   - search_files: walk directory for matching files
//!   - verify_move: re-read destination files, compare sha256 + size + mtime
//!
//! Not in W3a scope (deferred to W3b/W4):
//!   - MCP server wrapping
//!   - ro-only enforcement (caller's responsibility until MCP lands)
//!   - allowed_paths whitelist enforcement (caller's responsibility)

use crate::error::{KernelError, Result};
use crate::policy::transaction::{EffectManifest, FileSnapshot, PrepareToken, TransactionManager};
use crate::tools::fs_paths::canonicalize;
use crate::tools::fs_snapshot::snapshot_file;
use std::path::Path;

pub struct FilesystemTool {
    // Reserved for future config (allowed_paths, ro_only). Empty for now.
}

impl Default for FilesystemTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemTool {
    pub fn new() -> Self {
        Self {}
    }

    /// Phase 1 of move_files transaction.
    ///
    /// Snapshots each source file, checks the destination exists and is a dir,
    /// detects name conflicts at the destination, then asks the TransactionManager
    /// to issue a prepare_token. Returns the manifest + token + hash.
    pub fn prepare_move(
        &self,
        task_id: &str,
        step_id: &str,
        sources: &[&Path],
        destination: &Path,
        mgr: &TransactionManager,
    ) -> Result<PrepareMoveResult> {
        if sources.is_empty() {
            return Err(KernelError::Filesystem("no sources provided".to_string()));
        }

        // Destination must exist and be a directory.
        let dest_meta = std::fs::metadata(destination)
            .map_err(|e| KernelError::Filesystem(format!("destination missing: {}", e)))?;
        if !dest_meta.is_dir() {
            return Err(KernelError::Filesystem(
                "destination is not a directory".to_string(),
            ));
        }

        // Snapshot each source, accumulate conflicts.
        let mut snapshots: Vec<FileSnapshot> = Vec::with_capacity(sources.len());
        let mut conflicts: Vec<String> = Vec::new();
        let mut total_bytes: u64 = 0;

        for src in sources {
            let snap = snapshot_file(src)?;
            total_bytes += snap.size;
            snapshots.push(snap);

            // Check for destination conflict: dest / src.filename
            let filename = src
                .file_name()
                .ok_or_else(|| KernelError::Filesystem(format!("source has no filename: {}", src.display())))?
                .to_string_lossy()
                .to_string();
            let dest_path = destination.join(&filename);
            if dest_path.exists() {
                conflicts.push(canonicalize(&dest_path.to_string_lossy()));
            }
        }

        let manifest = EffectManifest {
            sources: snapshots,
            destination: canonicalize(&destination.to_string_lossy()),
            conflicts,
            total_bytes,
        };

        let token = mgr.prepare(task_id, step_id, &manifest)?;
        let preconditions_hash = token.preconditions_hash.clone();

        Ok(PrepareMoveResult {
            manifest,
            token,
            preconditions_hash,
        })
    }

    /// Phase 2 of move_files transaction.
    ///
    /// 1. Re-snapshot each source, recompute preconditions_hash, compare with stored.
    /// 2. Re-check destination conflicts (any new conflict → abort).
    /// 3. Ask TransactionManager to validate the token + hash (also enforces TTL).
    /// 4. Execute atomic-ish move (rename; fall back to copy+delete across volumes).
    /// 5. Return CommitMoveResult with succeeded=true + moved_paths.
    ///
    /// On any failure mid-move: attempt to roll back (move files back) and return Err.
    pub fn commit_move(
        &self,
        token: &PrepareToken,
        manifest: &EffectManifest,
        mgr: &TransactionManager,
    ) -> Result<CommitMoveResult> {
        // Re-snapshot sources and recompute hash via the manager.
        let mut fresh_snapshots: Vec<FileSnapshot> = Vec::with_capacity(manifest.sources.len());
        for snap in &manifest.sources {
            let path = std::path::PathBuf::from(&snap.canonical_path);
            let fresh = snapshot_file(&path)?;
            fresh_snapshots.push(fresh);
        }
        let fresh_manifest = EffectManifest {
            sources: fresh_snapshots.clone(),
            destination: manifest.destination.clone(),
            conflicts: manifest.conflicts.clone(),
            total_bytes: manifest.total_bytes,
        };

        // Token validation (checks stored hash + expiry).
        let commit_result = mgr.commit(token, &fresh_manifest)?;

        // Re-check destination conflicts (a new conflict appearing after prepare is TOCTOU).
        let dest_path = std::path::PathBuf::from(&manifest.destination);
        let mut new_conflicts: Vec<String> = Vec::new();
        for snap in &manifest.sources {
            let filename = std::path::Path::new(&snap.canonical_path)
                .file_name()
                .ok_or_else(|| KernelError::Filesystem("source path has no filename".to_string()))?
                .to_string_lossy()
                .to_string();
            let dest_target = dest_path.join(&filename);
            if dest_target.exists() {
                // Was this conflict already known at prepare time?
                let was_known = manifest
                    .conflicts
                    .iter()
                    .any(|c| c == &canonicalize(&dest_target.to_string_lossy()));
                if !was_known {
                    new_conflicts.push(canonicalize(&dest_target.to_string_lossy()));
                }
            }
        }
        if !new_conflicts.is_empty() {
            return Err(KernelError::Filesystem(format!(
                "new destination conflict appeared after prepare: {:?}",
                new_conflicts
            )));
        }

        // Execute moves with rollback tracking.
        let mut moved: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
        for snap in &manifest.sources {
            let src = std::path::PathBuf::from(&snap.canonical_path);
            let filename = src
                .file_name()
                .ok_or_else(|| KernelError::Filesystem("source path has no filename".to_string()))?
                .to_string_lossy()
                .to_string();
            let dest_target = dest_path.join(&filename);

            match std::fs::rename(&src, &dest_target) {
                Ok(_) => {
                    moved.push((src, dest_target));
                }
                Err(e) if e.raw_os_error() == Some(17) /* EXDEV: cross-device */ => {
                    // Fallback: copy + delete.
                    std::fs::copy(&src, &dest_target).map_err(|e| {
                        rollback_moves(&moved);
                        KernelError::Filesystem(format!("copy fallback failed: {}", e))
                    })?;
                    if let Err(e) = std::fs::remove_file(&src) {
                        rollback_moves(&moved);
                        return Err(KernelError::Filesystem(format!(
                            "post-copy delete failed: {}",
                            e
                        )));
                    }
                    moved.push((src, dest_target));
                }
                Err(e) => {
                    rollback_moves(&moved);
                    return Err(KernelError::Filesystem(format!("move failed: {}", e)));
                }
            }
        }

        Ok(CommitMoveResult {
            succeeded: true,
            moved_paths: moved,
            preconditions_recheck: commit_result.preconditions_recheck,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PrepareMoveResult {
    pub manifest: EffectManifest,
    pub token: PrepareToken,
    pub preconditions_hash: String,
}

#[derive(Debug, Clone)]
pub struct CommitMoveResult {
    pub succeeded: bool,
    pub moved_paths: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    pub preconditions_recheck: String,
}

/// Attempt to move already-moved files back to their original locations.
/// Best-effort; logs failures via tracing.
fn rollback_moves(moved: &[(std::path::PathBuf, std::path::PathBuf)]) {
    for (original_src, current_dest) in moved.iter().rev() {
        if let Err(e) = std::fs::rename(current_dest, original_src) {
            tracing::error!(
                "rollback failed: {} -> {}: {}",
                current_dest.display(),
                original_src.display(),
                e
            );
        }
    }
}
