//! AllowedPaths whitelist — V1.1 §4.4 step 1 + §8.1 mcp_servers.allowed_paths.
//!
//! Canonicalizes paths via `fs_paths::canonicalize` (pure string) and
//! checks prefix match against any allowed root. Backward slashes are
//! normalized so Windows paths match regardless of separator.
//!
//! # Known limitation (Unix)
//!
//! `fs_paths::canonicalize` strips the leading `/` from absolute Unix paths
//! (e.g., `/foo` becomes `foo`). This means a root of `/foo` would
//! spuriously match `/foobar/baz` on Unix because `foobar/baz` starts with
//! `foo/`. On Windows, the drive prefix (`c:`) preserves absoluteness, so
//! the bug does not manifest. This is tracked as spec issue #36 for V1.1.2
//! and should be fixed in `fs_paths::canonicalize` directly. For W3b PoC on
//! Windows, the whitelist is correct.

use crate::error::{KernelError, Result};
use crate::tools::fs_paths::canonicalize;

#[derive(Debug, Clone, Default)]
pub struct AllowedPaths {
    roots: Vec<String>, // each is canonicalized
}

impl AllowedPaths {
    pub fn new(roots: Vec<String>) -> Self {
        let canonical: Vec<String> = roots.iter().map(|r| canonicalize(r)).collect();
        Self { roots: canonical }
    }

    /// Check if `path` is under one of the allowed roots.
    pub fn check(&self, path: &std::path::Path) -> Result<()> {
        let canon = canonicalize(&path.to_string_lossy());
        for root in &self.roots {
            // Match if canon == root OR canon starts with root + "/".
            if canon == *root || canon.starts_with(&format!("{}/", root)) {
                return Ok(());
            }
        }
        Err(KernelError::PathNotAllowed(format!(
            "path {} not under any allowed root: {:?}",
            canon, self.roots
        )))
    }

    /// Returns true if no allowed_paths are configured (open access).
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}
