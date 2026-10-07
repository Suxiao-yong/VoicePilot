//! FilesystemTool — V1.1 §6.1, §6.2.
//!
//! W3a scope:
//!   - prepare_move: snapshot sources → EffectManifest → prepare_token
//!   - commit_move: re-check preconditions, execute move, return ToolResult
//!   - search_files: walk directory for matching files
//!   - verify_move: re-read destination files, compare sha256 + size + mtime
//!
//! Not in W3a scope (deferred to W4):
//!   - MCP server wrapping
//!   - ro-only enforcement (caller's responsibility until MCP lands)
//!
//! W3b: `allowed_paths` whitelist is enforced when `FilesystemTool::new_with_allowed_paths()`
//! is used; `new()` retains open access for backward compatibility.

use crate::error::{KernelError, Result};
use crate::policy::transaction::{EffectManifest, FileSnapshot, PrepareToken, TransactionManager};
use crate::tools::fs_paths::canonicalize;
use crate::tools::fs_snapshot::snapshot_file;
use std::path::Path;

pub struct FilesystemTool {
    allowed_paths: Option<crate::allowed_paths::AllowedPaths>,
}

impl Default for FilesystemTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemTool {
    pub fn new() -> Self {
        Self {
            allowed_paths: None,
        }
    }

    /// Construct a FilesystemTool that enforces `allowed_paths` on every
    /// source and destination. V1.1 §4.4 step 1 + §8.1 mcp_servers.allowed_paths.
    pub fn new_with_allowed_paths(allowed: crate::allowed_paths::AllowedPaths) -> Self {
        Self {
            allowed_paths: Some(allowed),
        }
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

        // V1.1 §4.4 step 1 + §8.1 allowed_paths enforcement.
        if let Some(allowed) = &self.allowed_paths {
            for src in sources {
                allowed.check(src)?;
            }
            allowed.check(destination)?;
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
                .ok_or_else(|| {
                    KernelError::Filesystem(format!("source has no filename: {}", src.display()))
                })?
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

    /// Strong Verifier — V1.1 §7.1.
    ///
    /// Re-reads each destination file, recomputes sha256 + size, compares with
    /// the original source snapshot in the manifest. Returns Strong evidence
    /// if ALL sources match. Returns Err on any mismatch (caller decides retry vs fail).
    pub fn verify_move(&self, manifest: &EffectManifest) -> Result<VerifyResult> {
        let dest_dir = std::path::PathBuf::from(&manifest.destination);
        for snap in &manifest.sources {
            let filename = std::path::Path::new(&snap.canonical_path)
                .file_name()
                .ok_or_else(|| KernelError::Filesystem("source path has no filename".to_string()))?
                .to_string_lossy()
                .to_string();
            let dest_target = dest_dir.join(&filename);

            let dest_meta =
                std::fs::metadata(&dest_target).map_err(|_| KernelError::Verification {
                    message: format!(
                        "destination file missing after move: {}",
                        dest_target.display()
                    ),
                })?;
            if !dest_meta.is_file() {
                return Err(KernelError::Verification {
                    message: format!(
                        "destination is not a regular file: {}",
                        dest_target.display()
                    ),
                });
            }
            if dest_meta.len() != snap.size {
                return Err(KernelError::Verification {
                    message: format!(
                        "size mismatch for {}: expected {} got {}",
                        dest_target.display(),
                        snap.size,
                        dest_meta.len()
                    ),
                });
            }

            let actual_hash = sha256_of_file(&dest_target)?;
            if actual_hash != snap.sha256 {
                return Err(KernelError::Verification {
                    message: format!(
                        "sha256 mismatch for {}: expected {} got {}",
                        dest_target.display(),
                        snap.sha256,
                        actual_hash
                    ),
                });
            }
        }

        Ok(VerifyResult {
            verified: true,
            evidence_strength: crate::toolresult::EvidenceStrength::Strong,
        })
    }

    /// Walk `root` recursively, return all files whose name matches `pattern`.
    ///
    /// Pattern is a simple glob: `*` matches any chars, `?` matches one char.
    /// Case-insensitive on Windows (matches NTFS filesystem behavior).
    /// Returns paths in walkdir's natural order (depth-first, directory then contents).
    pub fn search_files(&self, root: &Path, pattern: &str) -> Result<Vec<std::path::PathBuf>> {
        if let Some(allowed) = &self.allowed_paths {
            allowed.check(root)?;
        }
        let meta = std::fs::metadata(root)
            .map_err(|e| KernelError::Filesystem(format!("search root missing: {}", e)))?;
        if !meta.is_dir() {
            return Err(KernelError::Filesystem(
                "search root is not a directory".to_string(),
            ));
        }

        let mut out: Vec<std::path::PathBuf> = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry =
                entry.map_err(|e| KernelError::Filesystem(format!("walkdir error: {}", e)))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let filename = entry.file_name().to_string_lossy();
            if glob_matches(pattern, &filename) {
                out.push(entry.path().to_path_buf());
            }
        }
        Ok(out)
    }

    /// 检查 path 是否在 allowed_paths 白名单内(W6b-3a Task 2)。
    ///
    /// - 若 `allowed_paths` 为 None(开放访问),返回 Ok(())
    /// - 若 `allowed_paths` 为 Some,委托给 `AllowedPaths::check`
    ///
    /// 用于 diff_commands 等只读操作的安全校验,
    /// 不修改文件系统,只检查路径合法性。
    pub fn assert_path_allowed(&self, path: &Path) -> Result<()> {
        if let Some(allowed) = &self.allowed_paths {
            allowed.check(path)
        } else {
            Ok(())
        }
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

use crate::tools::fs_snapshot::FileSnapshot as FsSnap;
// Avoid pulling fs_snapshot's private sha256_of_file — re-declare a local one.
fn sha256_of_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(KernelError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf).map_err(KernelError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub verified: bool,
    pub evidence_strength: crate::toolresult::EvidenceStrength,
}

// Suppress unused-import warning for FsSnap (used for type inference in future tasks).
#[allow(dead_code)]
fn _touch_fssnap() -> FsSnap {
    unimplemented!()
}

/// Simple glob matcher: `*` = any chars (including zero), `?` = exactly one char.
/// Case-insensitive on Windows, case-sensitive elsewhere.
fn glob_matches(pattern: &str, input: &str) -> bool {
    #[cfg(windows)]
    {
        glob_matches_impl(
            pattern.to_lowercase().as_str(),
            input.to_lowercase().as_str(),
        )
    }
    #[cfg(not(windows))]
    {
        glob_matches_impl(pattern, input)
    }
}

fn glob_matches_impl(pattern: &str, input: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = input.chars().collect();
    glob_recursive(&p, 0, &s, 0)
}

fn glob_recursive(p: &[char], pi: usize, s: &[char], si: usize) -> bool {
    if pi == p.len() {
        return si == s.len();
    }
    match p[pi] {
        '*' => {
            // Try matching zero or more chars.
            for skip in 0..=(s.len() - si) {
                if glob_recursive(p, pi + 1, s, si + skip) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if si >= s.len() {
                return false;
            }
            glob_recursive(p, pi + 1, s, si + 1)
        }
        c => {
            if si >= s.len() || s[si] != c {
                return false;
            }
            glob_recursive(p, pi + 1, s, si + 1)
        }
    }
}

#[cfg(test)]
mod assert_path_allowed_tests {
    use super::*;
    use crate::allowed_paths::AllowedPaths;
    use std::path::Path;

    #[test]
    fn assert_path_allowed_open_access() {
        // new() 无 allowed_paths —— 开放访问,任何路径都 Ok
        let tool = FilesystemTool::new();
        let path = Path::new("E:/definitely_nonexistent/path.txt");
        assert!(tool.assert_path_allowed(path).is_ok());
    }

    #[test]
    fn assert_path_allowed_whitelist_pass() {
        // 用 tempdir 作为 allowed root
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let allowed = AllowedPaths::new(vec![tmp_path]);
        let tool = FilesystemTool::new_with_allowed_paths(allowed);

        let path_inside = tmp.path().join("file.txt");
        assert!(tool.assert_path_allowed(&path_inside).is_ok());
    }

    #[test]
    fn assert_path_allowed_whitelist_block() {
        let allowed = AllowedPaths::new(vec!["C:/safe_area".to_string()]);
        let tool = FilesystemTool::new_with_allowed_paths(allowed);

        // E:/definitely_nonexistent 不在 C:/safe_area 下
        let path_outside = Path::new("E:/definitely_nonexistent/path.txt");
        let result = tool.assert_path_allowed(path_outside);
        assert!(matches!(
            result,
            Err(crate::error::KernelError::PathNotAllowed(_))
        ));
    }
}
