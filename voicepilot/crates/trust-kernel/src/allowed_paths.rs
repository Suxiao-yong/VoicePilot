//! AllowedPaths whitelist — V1.1 §4.4 step 1 + §8.1 mcp_servers.allowed_paths.
//!
//! Canonicalizes paths via `fs_paths::canonicalize` (pure string) and
//! checks prefix match against any allowed root. Backward slashes are
//! normalized so Windows paths match regardless of separator.
//!
//! Project is Windows-only (user decision 2026-07-26); the Windows drive
//! prefix (`c:`) preserved by `fs_paths::canonicalize` keeps absoluteness,
//! so the whitelist is correct on Windows.

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
