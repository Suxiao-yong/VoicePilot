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
}

#[derive(Debug, Clone)]
pub struct PrepareMoveResult {
    pub manifest: EffectManifest,
    pub token: PrepareToken,
    pub preconditions_hash: String,
}
