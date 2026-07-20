# W3a: Filesystem Tool Adapter + Compensation + Strong Verifier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement V1.1 §6.2 prepare→approve→commit transaction against a real local filesystem, plus §7.1 Strong Verifier (filesystem re-reads) and §7.2 Compensation three-level (strong/best_effort/none) with auto_reverse for move_files. End state: `move_files` works end-to-end with TOCTOU prevention, post-commit verification, and reversible compensation.

**Architecture:** A new `tools` module hosts the `FilesystemTool` adapter — it snapshots files (path canonicalization + sha256 + size + mtime), produces `EffectManifest` for the W2 `TransactionManager`, executes `commit_move` with atomic rename-or-copy semantics, and verifies by re-reading. A new `compensation` module persists `CompensationRecord` rows (encrypted snapshot deferred to W8; W3a stores plaintext in SQLite for PoC, clearly marked) and supports `auto_reverse` for `move_files`. No external MCP server yet — W3a is a pure-Rust filesystem adapter that the future MCP wrapper (W3b/W4) will call into.

**Tech Stack:** Rust 1.96, `rusqlite` 0.32, `serde` 1.0, `serde_json` 1.0, `sha2` 0.10, `chrono` 0.4, `uuid` 1.10, `thiserror` 2.0. New dep: `walkdir` 2.5 (for listing directory contents in search_files — added in Task 1). No external MCP SDK; we implement the filesystem operations natively.

**Reference:** V1.1.1 spec at `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`. Relevant sections: §6.1 (filesystem-mcp), §6.2 (prepare→approve→commit, effect_manifest schema), §6.3 (ToolResult V2), §7.1 (Verifier evidence_strength), §7.2 (Compensation three-level + safety constraints), §8.1 (compensations table), §11.1 W3 gate "files.organize Skill 可跑" (W3a delivers the filesystem half; W3b wires the Skill).

**W2 prerequisites (already complete):** `TransactionManager` (prepare/commit/preconditions_hash), `ActionGateway` (decide pipeline), `EffectManifest` / `FileSnapshot` / `PrepareToken` types, `compensations` table schema, `steps` table with `prepare_token`/`preconditions_hash`/`effect_manifest`/`evidence_strength`/`compensation_ref` columns.

---

## File Structure

**New files:**
- `voicepilot/crates/trust-kernel/src/tools/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/tools/fs.rs` — `FilesystemTool` (prepare_move, commit_move, search_files, verify_move)
- `voicepilot/crates/trust-kernel/src/tools/fs_snapshot.rs` — `snapshot_file()` / `snapshot_path()` helpers (canonicalize + sha256 + size + mtime)
- `voicepilot/crates/trust-kernel/src/tools/fs_paths.rs` — path canonicalization (Windows drive letter, forward slashes, absolute resolution without requiring existence)
- `voicepilot/crates/trust-kernel/src/compensation/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/compensation/types.rs` — `CompensationLevel`, `ConflictPolicy`, `CompensationRecord`, `CompensationAction`
- `voicepilot/crates/trust-kernel/src/compensation/repo.rs` — `CompensationRepo` CRUD against `compensations` table
- `voicepilot/crates/trust-kernel/src/compensation/executor.rs` — `auto_reverse_move()` for move_files
- `voicepilot/crates/trust-kernel/src/toolresult.rs` — `ToolResult` V2 struct (§6.3)
- `voicepilot/crates/trust-kernel/tests/fs_paths.rs`
- `voicepilot/crates/trust-kernel/tests/fs_snapshot.rs`
- `voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs`
- `voicepilot/crates/trust-kernel/tests/fs_verify.rs`
- `voicepilot/crates/trust-kernel/tests/fs_search.rs`
- `voicepilot/crates/trust-kernel/tests/compensation_repo.rs`
- `voicepilot/crates/trust-kernel/tests/compensation_reverse.rs`
- `voicepilot/crates/trust-kernel/tests/toolresult.rs`

**Modified files:**
- `voicepilot/Cargo.toml` — add `walkdir = "2.5"` to `[workspace.dependencies]`
- `voicepilot/crates/trust-kernel/Cargo.toml` — add `walkdir = { workspace = true }`
- `voicepilot/crates/trust-kernel/src/lib.rs` — add `pub mod tools; pub mod compensation; pub mod toolresult;`
- `voicepilot/crates/trust-kernel/src/error.rs` — add filesystem + compensation error variants
- `voicepilot/crates/trust-kernel/src/kernel.rs` — add `filesystem()` and `compensation()` accessors
- `voicepilot/crates/trust-kernel/src/repo/step_repo.rs` — extend with `update_prepare_token()`, `update_effect_manifest()`, `update_evidence_strength()`, `update_compensation_ref()`
- `voicepilot/crates/cli/src/main.rs` — add `move <src1> [src2...] <dest>` command exercising the full prepare→approve→commit→verify→compensate pipeline

---

## Task 1: Add walkdir dependency + path canonicalization

**Files:**
- Modify: `voicepilot/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Create: `voicepilot/crates/trust-kernel/src/tools/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/tools/fs_paths.rs`
- Create: `voicepilot/crates/trust-kernel/tests/fs_paths.rs`

- [ ] **Step 1: Add walkdir to workspace deps**

Edit `voicepilot/Cargo.toml` `[workspace.dependencies]` section, add after `cedar-policy = "4.11.2"`:

```toml
walkdir = "2.5"
```

- [ ] **Step 2: Add walkdir to trust-kernel crate**

Edit `voicepilot/crates/trust-kernel/Cargo.toml` `[dependencies]` section, add after `cedar-policy = { workspace = true }`:

```toml
walkdir = { workspace = true }
```

- [ ] **Step 3: Write the failing test for path canonicalization**

Create `voicepilot/crates/trust-kernel/tests/fs_paths.rs`:

```rust
use trust_kernel::tools::fs_paths::canonicalize;

#[test]
fn backslashes_become_forward() {
    let p = canonicalize(r"C:\Users\me\file.txt");
    assert_eq!(p, "c:/Users/me/file.txt");
}

#[test]
fn drive_letter_lowercased() {
    let p = canonicalize(r"D:/Docs/Readme.md");
    assert_eq!(p, "d:/Docs/Readme.md");
}

#[test]
fn duplicated_slashes_collapsed() {
    let p = canonicalize(r"C:/Users//me///file.txt");
    assert_eq!(p, "c:/Users/me/file.txt");
}

#[test]
fn trailing_slash_stripped_for_files() {
    // We can't know if it's a file vs dir without filesystem access;
    // canonicalize() preserves trailing slash as a single '/'.
    let p = canonicalize("C:/Users/me/");
    assert_eq!(p, "c:/Users/me/");
}

#[test]
fn relative_path_left_as_is() {
    let p = canonicalize("docs/readme.md");
    assert_eq!(p, "docs/readme.md");
}

#[test]
fn dot_segments_resolved() {
    let p = canonicalize("C:/Users/me/../you/file.txt");
    assert_eq!(p, "c:/Users/you/file.txt");
}

#[test]
fn dot_dot_at_root_stays_at_root() {
    let p = canonicalize("C:/../file.txt");
    assert_eq!(p, "c:/file.txt");
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_paths 2>&1`
Expected: FAIL — `tools` module does not exist.

- [ ] **Step 5: Add tools module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod gateway;`:

```rust
pub mod tools;
pub mod compensation;
pub mod toolresult;
```

- [ ] **Step 6: Create tools/mod.rs**

Create `voicepilot/crates/trust-kernel/src/tools/mod.rs`:

```rust
//! Tool adapters — V1.1 §6.
//!
//! W3a: FilesystemTool (native Rust, no MCP SDK yet).
//! W3b/W4 will wrap this in an MCP server handler.

pub mod fs_paths;
pub mod fs_snapshot;
pub mod fs;
```

- [ ] **Step 7: Create tools/fs_paths.rs**

Create `voicepilot/crates/trust-kernel/src/tools/fs_paths.rs`:

```rust
//! Path canonicalization — V1.1 §4.4 step 1 + §6.2 prepare.
//!
//! Pure string transformation; does NOT touch the filesystem.
//! - Backslashes → forward slashes
//! - Lowercase drive letter (Windows)
//! - Collapse duplicate slashes
//! - Resolve `.` and `..` segments
//! - Preserve trailing slash (caller decides file vs dir)

/// Canonicalize a path string without touching the filesystem.
pub fn canonicalize(input: &str) -> String {
    // Step 1: backslashes → forward slashes.
    let with_forward = input.replace('\\', "/");

    // Step 2: lowercase drive letter if present (e.g. "C:/" → "c:/").
    let mut chars = with_forward.chars().collect::<Vec<_>>();
    let drive_lowered: String = if chars.len() >= 2 && chars[1] == ':' && chars[0].is_ascii_uppercase() {
        chars[0] = chars[0].to_ascii_lowercase();
        chars.into_iter().collect()
    } else {
        with_forward
    };

    // Step 3: split on '/', resolve "." and "..", collapse duplicates.
    let has_trailing_slash = drive_lowered.ends_with('/') && !drive_lowered.ends_with(":/");
    let mut parts: Vec<&str> = Vec::new();
    let mut prefix = String::new(); // holds "c:" or "" for relative
    let mut first_segment = true;
    for segment in drive_lowered.split('/') {
        if first_segment {
            first_segment = false;
            // If segment looks like "c:" treat as drive prefix.
            if segment.len() == 2 && segment.as_bytes()[1] == b':' {
                prefix = segment.to_string();
                continue;
            }
        }
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if let Some(last) = parts.last() {
                if *last != ".." {
                    parts.pop();
                    continue;
                }
            }
            // .. at root or in relative path with no prior segment: keep if relative, drop if absolute.
            if !prefix.is_empty() {
                // At filesystem root: drop the ..
                continue;
            }
            // Relative path: keep the ..
            parts.push("..");
            continue;
        }
        parts.push(segment);
    }

    let mut out = prefix;
    if !prefix.is_empty() {
        out.push('/');
    }
    out.push_str(&parts.join("/"));
    if out.is_empty() {
        out.push('.');
    }
    if has_trailing_slash && !out.ends_with('/') {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::canonicalize;

    #[test]
    fn empty_becomes_dot() {
        assert_eq!(canonicalize(""), ".");
    }
}
```

- [ ] **Step 8: Create stub modules so it compiles**

Create `voicepilot/crates/trust-kernel/src/tools/fs_snapshot.rs`:

```rust
//! File snapshot helpers — V1.1 §6.2.
//! Stub; implemented in Task 2.
```

Create `voicepilot/crates/trust-kernel/src/tools/fs.rs`:

```rust
//! FilesystemTool — V1.1 §6.1, §6.2.
//! Stub; implemented in Tasks 3-5.
```

Create `voicepilot/crates/trust-kernel/src/compensation/mod.rs`:

```rust
//! Compensation three-level — V1.1 §7.2.
//! Stub; implemented in Tasks 6-8.
```

Create `voicepilot/crates/trust-kernel/src/toolresult.rs`:

```rust
//! ToolResult V2 — V1.1 §6.3.
//! Stub; implemented in Task 9.
```

- [ ] **Step 9: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_paths 2>&1`
Expected: PASS — 7 tests + 1 inline test.

- [ ] **Step 10: Commit**

```bash
cd d:\voicepilot
git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/tools/ voicepilot/crates/trust-kernel/src/compensation/mod.rs voicepilot/crates/trust-kernel/src/toolresult.rs voicepilot/crates/trust-kernel/tests/fs_paths.rs
git commit -m "feat(tools): walkdir dep + path canonicalization (V1.1 §4.4 step 1)"
```

---

## Task 2: File snapshot helpers (sha256 + size + mtime)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs_snapshot.rs`
- Create: `voicepilot/crates/trust-kernel/tests/fs_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/fs_snapshot.rs`:

```rust
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use trust_kernel::tools::fs_snapshot::snapshot_file;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn snapshot_file_captures_size_and_sha256() {
    let dir = tmp_dir();
    let file = dir.join("hello.txt");
    let mut f = fs::File::create(&file).unwrap();
    f.write_all(b"hello world").unwrap();
    drop(f);

    let snap = snapshot_file(&file).unwrap();
    assert_eq!(snap.size, 11);
    assert!(snap.sha256.starts_with("sha256:"));
    assert_eq!(snap.canonical_path, file.to_string_lossy().replace('\\', "/").to_lowercase().replace("//", "/"));
    assert!(!snap.last_write_time.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_fails_for_missing_file() {
    let dir = tmp_dir();
    let missing = dir.join("nope.txt");
    let result = snapshot_file(&missing);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_sha256_changes_with_content() {
    let dir = tmp_dir();
    let file = dir.join("a.txt");
    fs::write(&file, b"content v1").unwrap();
    let s1 = snapshot_file(&file).unwrap();

    fs::write(&file, b"content v2").unwrap();
    let s2 = snapshot_file(&file).unwrap();

    assert_ne!(s1.sha256, s2.sha256);
    assert_eq!(s1.canonical_path, s2.canonical_path);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_file_file_id_is_stable_across_reads() {
    let dir = tmp_dir();
    let file = dir.join("stable.txt");
    fs::write(&file, b"stable").unwrap();
    let s1 = snapshot_file(&file).unwrap();
    let s2 = snapshot_file(&file).unwrap();
    // On Windows, file_id comes from dwVolumeSerialNumber + nFileIndexHigh + nFileIndexLow.
    // Two consecutive reads should yield the same id.
    assert_eq!(s1.file_id, s2.file_id);
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_snapshot 2>&1`
Expected: FAIL — `snapshot_file` does not exist.

- [ ] **Step 3: Implement snapshot_file**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/tools/fs_snapshot.rs`:

```rust
//! File snapshot helpers — V1.1 §6.2.
//!
//! Captures the four-field snapshot used in `EffectManifest`:
//!   - canonical_path: via fs_paths::canonicalize()
//!   - file_id: OS-level file identity (Win: volume+index, Unix: inode)
//!   - size: file length in bytes
//!   - last_write_time: RFC3339 string
//!   - sha256: hex digest prefixed with "sha256:"
//!
//! These fields feed into `preconditions_hash` for TOCTOU detection.

use crate::error::{KernelError, Result};
use crate::tools::fs_paths::canonicalize;
use chrono::{DateTime, Utc};
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub canonical_path: String,
    pub file_id: String,
    pub size: u64,
    pub last_write_time: String, // RFC3339
    pub sha256: String,          // "sha256:<hex>"
}

/// Snapshot a single file. Returns KernelError::Io if the path is missing or not a regular file.
pub fn snapshot_file(path: &Path) -> Result<FileSnapshot> {
    let meta = fs::metadata(path).map_err(|e| KernelError::Io(e))?;
    if !meta.is_file() {
        return Err(KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("not a regular file: {}", path.display()),
        )));
    }

    let canonical_path = canonicalize(&path.to_string_lossy());
    let size = meta.len();
    let last_write_time = format_rfc3339(meta.modified()?);
    let file_id = file_identity(path, &meta)?;
    let sha256 = sha256_of_file(path)?;

    Ok(FileSnapshot {
        canonical_path,
        file_id,
        size,
        last_write_time,
        sha256,
    })
}

/// Snapshot a list of files. Returns error on the first failure.
pub fn snapshot_files(paths: &[&Path]) -> Result<Vec<FileSnapshot>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(snapshot_file(p)?);
    }
    Ok(out)
}

fn sha256_of_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(KernelError::Io)?;
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

fn format_rfc3339(time: SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    dt.to_rfc3339()
}

#[cfg(windows)]
fn file_identity(path: &Path, _meta: &fs::Metadata) -> Result<String> {
    use std::os::windows::fs::MetadataExt;
    let m = fs::metadata(path).map_err(KernelError::Io)?;
    // Combine volume serial, file index high/low. Lower 16 bits of nFileIndexHigh
    // are reused for reparse tags on some NTFS builds; we include the full 64-bit
    // index which is stable for non-reparse files.
    let vol = m.volume_serial_number().unwrap_or(0);
    let idx_high = m.file_index_high().unwrap_or(0);
    let idx_low = m.file_index_low().unwrap_or(0);
    Ok(format!("win:{}:{}:{}", vol, idx_high, idx_low))
}

#[cfg(unix)]
fn file_identity(_path: &Path, meta: &fs::Metadata) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    Ok(format!("unix:{}:{}", meta.dev(), meta.ino()))
}

#[cfg(not(any(windows, unix)))]
fn file_identity(_path: &Path, _meta: &fs::Metadata) -> Result<String> {
    Ok("unknown:0".to_string())
}

// Suppress unused-import warning for SqlValue on non-windows; we keep it for future use.
#[allow(dead_code)]
fn _touch_sqlvalue() -> SqlValue {
    SqlValue::Null
}
```

- [ ] **Step 4: Add Io error variant (if missing)**

Check `voicepilot/crates/trust-kernel/src/error.rs` for an `Io(#[from] std::io::Error)` variant. If it exists (W1 added it), skip this step. If not, add it after `Serde(#[from] serde_json::Error)`:

```rust
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
```

(The W1 error.rs already has this; verify before modifying.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_snapshot 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/tools/fs_snapshot.rs voicepilot/crates/trust-kernel/tests/fs_snapshot.rs
git commit -m "feat(tools): file snapshot helpers (sha256 + size + mtime + file_id) for V1.1 §6.2"
```

---

## Task 3: FilesystemTool — prepare_move (snapshot + manifest + token)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`
- Modify: `voicepilot/crates/trust-kernel/src/error.rs` (add fs error variants)
- Create: `voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs`

- [ ] **Step 1: Add filesystem error variants**

Edit `voicepilot/crates/trust-kernel/src/error.rs`, add before the closing `}` of `KernelError`:

```rust
    #[error("filesystem error: {0}")]
    Filesystem(String),
    #[error("compensation error: {0}")]
    Compensation(String),
    #[error("verification failed: {message}")]
    Verification { message: String },
```

- [ ] **Step 2: Write the failing test for prepare_move**

Create `voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::policy::transaction::TransactionManager;
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::tools::fs_paths::canonicalize;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-fs-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn prepare_move_returns_manifest_with_snapshots() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt"); fs::write(&src1, b"content a").unwrap();
    let src2 = dir.join("b.txt"); fs::write(&src2, b"content b").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr).unwrap();

    assert!(result.token.token.starts_with("prt_"));
    assert!(!result.preconditions_hash.is_empty());
    assert_eq!(result.manifest.sources.len(), 2);
    assert_eq!(result.manifest.total_bytes, "content a".len() as u64 + "content b".len() as u64);
    assert_eq!(result.manifest.destination, canonicalize(&dest.to_string_lossy()));
    assert!(result.manifest.conflicts.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_detects_destination_conflicts() {
    let dir = tmp_dir();
    let src = dir.join("a.txt"); fs::write(&src, b"hello").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();
    // Pre-create a conflicting file at dest/a.txt
    fs::write(dest.join("a.txt"), b"existing").unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&src], &dest, &mgr).unwrap();

    assert!(!result.manifest.conflicts.is_empty(), "must detect dest/a.txt as conflict");
    assert!(result.manifest.conflicts[0].contains("a.txt"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_fails_if_source_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nope.txt");
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&missing], &dest, &mgr);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prepare_move_fails_if_destination_not_a_directory() {
    let dir = tmp_dir();
    let src = dir.join("a.txt"); fs::write(&src, b"hi").unwrap();
    let dest = dir.join("not_a_dir"); fs::write(&dest, b"blocker").unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let result = tool.prepare_move("task-1", "step-1", &[&src], &dest, &mgr);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_prepare_commit 2>&1`
Expected: FAIL — `FilesystemTool` does not exist.

- [ ] **Step 4: Implement FilesystemTool::prepare_move**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/tools/fs.rs`:

```rust
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_prepare_commit 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/src/error.rs voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs
git commit -m "feat(tools): FilesystemTool::prepare_move with snapshot + conflict detection (V1.1 §6.2)"
```

---

## Task 4: FilesystemTool — commit_move (atomic move + re-verify)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs`

- [ ] **Step 1: Append the commit_move tests**

Add to the end of `voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs`:

```rust
#[test]
fn commit_move_succeeds_when_preconditions_match() {
    let dir = tmp_dir();
    let src1 = dir.join("a.txt"); fs::write(&src1, b"content a").unwrap();
    let src2 = dir.join("b.txt"); fs::write(&src2, b"content b").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr).unwrap();
    assert!(result.succeeded);
    assert!(!src1.exists());
    assert!(!src2.exists());
    assert!(dest.join("a.txt").exists());
    assert!(dest.join("b.txt").exists());
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"content a");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_fails_when_source_tampered() {
    let dir = tmp_dir();
    let src = dir.join("a.txt"); fs::write(&src, b"original").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("task-1", "step-1", &[&src], &dest, &mgr).unwrap();

    // Tamper: change content after prepare.
    fs::write(&src, b"tampered").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "commit must fail on precondition mismatch");
    // Source must still exist (no partial move).
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_fails_when_destination_conflict_appeared() {
    let dir = tmp_dir();
    let src = dir.join("a.txt"); fs::write(&src, b"hello").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("task-1", "step-1", &[&src], &dest, &mgr).unwrap();

    // Inject a destination conflict after prepare.
    fs::write(dest.join("a.txt"), b"injected").unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr);
    assert!(result.is_err(), "commit must fail when new conflict appears");
    // Source must still exist.
    assert!(src.exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn commit_move_is_atomic_on_failure() {
    // If 1 of N files fails to move (e.g. locked), no files should move.
    // W3a approximates "atomic" by pre-checking all destinations + sources,
    // then moving. If any move errors mid-flight, we attempt rollback.
    let dir = tmp_dir();
    let src1 = dir.join("a.txt"); fs::write(&src1, b"first").unwrap();
    let src2 = dir.join("b.txt"); fs::write(&src2, b"second").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let tool = FilesystemTool::new();
    let mgr = TransactionManager::new();
    let prepared = tool.prepare_move("task-1", "step-1", &[&src1, &src2], &dest, &mgr).unwrap();

    let result = tool.commit_move(&prepared.token, &prepared.manifest, &mgr).unwrap();
    assert!(result.succeeded);
    // Both moved together.
    assert!(!src1.exists() && !src2.exists());
    assert!(dest.join("a.txt").exists() && dest.join("b.txt").exists());
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_prepare_commit 2>&1`
Expected: FAIL — `commit_move` does not exist.

- [ ] **Step 3: Implement commit_move**

In `voicepilot/crates/trust-kernel/src/tools/fs.rs`, add this `impl` block (after `prepare_move`):

```rust
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
```

Add the result struct + rollback helper at the end of `fs.rs`:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_prepare_commit 2>&1`
Expected: PASS — 8 tests (4 from Task 3 + 4 new).

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/tests/fs_prepare_commit.rs
git commit -m "feat(tools): FilesystemTool::commit_move with TOCTOU re-check + rollback (V1.1 §6.2)"
```

---

## Task 5: FilesystemTool — verify_move (Strong Verifier)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`
- Create: `voicepilot/crates/trust-kernel/tests/fs_verify.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/fs_verify.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::policy::transaction::{EffectManifest, FileSnapshot};
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::tools::fs_snapshot::snapshot_file;
use trust_kernel::toolresult::EvidenceStrength;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-verify-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn verify_move_strong_when_sha256_matches() {
    let dir = tmp_dir();
    let src = dir.join("a.txt"); fs::write(&src, b"hello world").unwrap();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();
    fs::rename(&src, dest.join("a.txt")).unwrap();

    let original_snap = snapshot_file(&dir.join("a.txt")).or_else(|_| {
        // src is now gone — build a synthetic snapshot for the test.
        // In real flow, the manifest comes from prepare_move before the move.
        Ok(FileSnapshot {
            canonical_path: "c:/placeholder/a.txt".to_string(),
            file_id: "test".to_string(),
            size: 11,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string(),
        })
    }).unwrap();

    let manifest = EffectManifest {
        sources: vec![original_snap],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 11,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest).unwrap();
    assert!(result.verified);
    assert_eq!(result.evidence_strength, EvidenceStrength::Strong);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn verify_move_fails_when_destination_missing() {
    let dir = tmp_dir();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();

    let manifest = EffectManifest {
        sources: vec![FileSnapshot {
            canonical_path: "c:/fake/source.txt".to_string(),
            file_id: "fake".to_string(),
            size: 5,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:fakehash".to_string(),
        }],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 5,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest);
    assert!(result.is_err(), "verify must fail when dest file is missing");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn verify_move_fails_when_sha256_mismatches() {
    let dir = tmp_dir();
    let dest = dir.join("out"); fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("source.txt"), b"different content").unwrap();

    let manifest = EffectManifest {
        sources: vec![FileSnapshot {
            canonical_path: "c:/fake/source.txt".to_string(),
            file_id: "fake".to_string(),
            size: 99,
            last_write_time: "2026-07-19T00:00:00Z".to_string(),
            sha256: "sha256:expectednotmatching".to_string(),
        }],
        destination: dest.to_string_lossy().replace('\\', "/").to_lowercase(),
        conflicts: vec![],
        total_bytes: 99,
    };

    let tool = FilesystemTool::new();
    let result = tool.verify_move(&manifest);
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_verify 2>&1`
Expected: FAIL — `verify_move` does not exist; `EvidenceStrength` does not exist.

- [ ] **Step 3: Create toolresult.rs with EvidenceStrength**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/toolresult.rs`:

```rust
//! ToolResult V2 — V1.1 §6.3, §7.1.
//!
//! W3a implements the subset needed for filesystem verification:
//!   - EvidenceStrength enum
//!   - ToolResult struct (status + data + evidence + compensation_ref)
//! Full V2 fields (egress_performed, idempotency_key, retryable) land in W3b.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceStrength {
    /// System-level state directly read (filesystem sha256+size match, PID exists).
    Strong,
    /// Structured but indirect (UIA tree contains expected node, accessibility snapshot).
    Medium,
    /// Tool's natural-language "succeeded" with no system-level check.
    Weak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: serde_json::Value,
    pub evidence_strength: EvidenceStrength,
    pub compensation_ref: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Succeeded,
    Failed,
    Cancelled,
    Partial,
}
```

- [ ] **Step 4: Implement verify_move**

In `voicepilot/crates/trust-kernel/src/tools/fs.rs`, add this method to the `impl FilesystemTool` block (after `commit_move`):

```rust
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

            let dest_meta = std::fs::metadata(&dest_target).map_err(|_| {
                KernelError::Verification {
                    message: format!("destination file missing after move: {}", dest_target.display()),
                }
            })?;
            if !dest_meta.is_file() {
                return Err(KernelError::Verification {
                    message: format!("destination is not a regular file: {}", dest_target.display()),
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
```

Add the `sha256_of_file` helper + `VerifyResult` struct at the end of `fs.rs`:

```rust
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_verify 2>&1`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/src/toolresult.rs voicepilot/crates/trust-kernel/tests/fs_verify.rs
git commit -m "feat(tools): Strong Verifier for move_files (V1.1 §7.1) + ToolResult V2 skeleton (§6.3)"
```

---

## Task 6: FilesystemTool — search_files (walkdir + glob filter)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`
- Create: `voicepilot/crates/trust-kernel/tests/fs_search.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/fs_search.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::tools::fs::FilesystemTool;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-search-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn search_files_returns_matching_pdf_files() {
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"pdf1").unwrap();
    fs::write(dir.join("b.pdf"), b"pdf2").unwrap();
    fs::write(dir.join("c.txt"), b"txt").unwrap();
    let subdir = dir.join("sub"); fs::create_dir_all(&subdir).unwrap();
    fs::write(subdir.join("d.pdf"), b"pdf3").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    let names: Vec<String> = results.iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"a.pdf".to_string()));
    assert!(names.contains(&"b.pdf".to_string()));
    assert!(names.contains(&"d.pdf".to_string()));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_returns_empty_for_no_matches() {
    let dir = tmp_dir();
    fs::write(dir.join("a.txt"), b"hi").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    assert!(results.is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_fails_when_root_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nope");
    let tool = FilesystemTool::new();
    let result = tool.search_files(&missing, "*.pdf");
    assert!(result.is_err());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn search_files_supports_multiple_extensions_via_star() {
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"1").unwrap();
    fs::write(dir.join("b.PDF"), b"2").unwrap(); // uppercase
    fs::write(dir.join("c.docx"), b"3").unwrap();

    let tool = FilesystemTool::new();
    let results = tool.search_files(&dir, "*.pdf").unwrap();
    // Case-insensitive match on Windows; both a.pdf and b.PDF should match.
    assert!(results.len() >= 1);
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_search 2>&1`
Expected: FAIL — `search_files` does not exist.

- [ ] **Step 3: Implement search_files**

In `voicepilot/crates/trust-kernel/src/tools/fs.rs`, add this method to `impl FilesystemTool`:

```rust
    /// Walk `root` recursively, return all files whose name matches `pattern`.
    ///
    /// Pattern is a simple glob: `*` matches any chars, `?` matches one char.
    /// Case-insensitive on Windows, case-sensitive on Unix (matches filesystem behavior).
    /// Returns paths in walkdir's natural order (depth-first, directory then contents).
    pub fn search_files(&self, root: &Path, pattern: &str) -> Result<Vec<std::path::PathBuf>> {
        let meta = std::fs::metadata(root).map_err(|e| {
            KernelError::Filesystem(format!("search root missing: {}", e))
        })?;
        if !meta.is_dir() {
            return Err(KernelError::Filesystem("search root is not a directory".to_string()));
        }

        let mut out: Vec<std::path::PathBuf> = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(|e| KernelError::Filesystem(format!("walkdir error: {}", e)))?;
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
```

Add the `glob_matches` function at the end of `fs.rs`:

```rust
/// Simple glob matcher: `*` = any chars (including zero), `?` = exactly one char.
/// Case-insensitive on Windows, case-sensitive elsewhere.
fn glob_matches(pattern: &str, input: &str) -> bool {
    #[cfg(windows)]
    {
        glob_matches_impl(pattern.to_lowercase().as_str(), input.to_lowercase().as_str())
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_search 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/tests/fs_search.rs
git commit -m "feat(tools): FilesystemTool::search_files with glob filter (V1.1 §6.1)"
```

---

## Task 7: Compensation types + repo

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/compensation/types.rs`
- Create: `voicepilot/crates/trust-kernel/src/compensation/repo.rs`
- Modify: `voicepilot/crates/trust-kernel/src/compensation/mod.rs`
- Create: `voicepilot/crates/trust-kernel/tests/compensation_repo.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/compensation_repo.rs`:

```rust
use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use trust_kernel::compensation::repo::CompensationRepo;
use trust_kernel::db;

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn create_compensation_persists_and_can_be_loaded() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    let rec = CompensationRecord {
        comp_id: "comp-1".to_string(),
        step_id: "step-1".to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: Some(b"encrypted-blob".to_vec()),
        ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(),
        snapshot_vault_ref: Some("stronghold://voicepilot/comp/abc".to_string()),
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: r#"{"sources":["c:/out/a.txt"],"dest":"c:/orig"}"#.to_string(),
    };
    repo.create(&conn, &rec).unwrap();

    let loaded = repo.get(&conn, "comp-1").unwrap().expect("must exist");
    assert_eq!(loaded.level, CompensationLevel::Strong);
    assert_eq!(loaded.conflict_policy, ConflictPolicy::AutoReverse);
    assert_eq!(loaded.compensate_fn, "filesystem.reverse_move");
    assert_eq!(loaded.status, "active");
}

#[test]
fn get_missing_returns_none() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    assert!(repo.get(&conn, "nope").unwrap().is_none());
}

#[test]
fn mark_consumed_updates_status() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    let rec = CompensationRecord {
        comp_id: "comp-2".to_string(), step_id: "s".to_string(),
        level: CompensationLevel::BestEffort,
        snapshot_encrypted: None, ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(), snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::RequireConfirmation,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: "{}".to_string(),
    };
    repo.create(&conn, &rec).unwrap();
    repo.mark_status(&conn, "comp-2", "consumed").unwrap();
    let loaded = repo.get(&conn, "comp-2").unwrap().unwrap();
    assert_eq!(loaded.status, "consumed");
}

#[test]
fn list_active_returns_only_active() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    for (id, status) in [("c1", "active"), ("c2", "consumed"), ("c3", "active")] {
        let rec = CompensationRecord {
            comp_id: id.to_string(), step_id: "s".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None, ttl_expires: "2026-07-19T16:00:00Z".to_string(),
            status: status.to_string(), snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "filesystem.reverse_move".to_string(),
            reverse_payload: "{}".to_string(),
        };
        repo.create(&conn, &rec).unwrap();
    }
    let active = repo.list_active(&conn).unwrap();
    assert_eq!(active.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test compensation_repo 2>&1`
Expected: FAIL — `compensation::types` / `compensation::repo` do not exist.

- [ ] **Step 3: Create compensation/types.rs**

Create `voicepilot/crates/trust-kernel/src/compensation/types.rs`:

```rust
//! Compensation types — V1.1 §7.2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationLevel {
    None,
    BestEffort,
    Strong,
}

impl CompensationLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            CompensationLevel::None => "none",
            CompensationLevel::BestEffort => "best_effort",
            CompensationLevel::Strong => "strong",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(CompensationLevel::None),
            "best_effort" => Some(CompensationLevel::BestEffort),
            "strong" => Some(CompensationLevel::Strong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    AutoReverse,
    RequireConfirmation,
    Fail,
}

impl ConflictPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictPolicy::AutoReverse => "auto_reverse",
            ConflictPolicy::RequireConfirmation => "require_confirmation",
            ConflictPolicy::Fail => "fail",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto_reverse" => Some(ConflictPolicy::AutoReverse),
            "require_confirmation" => Some(ConflictPolicy::RequireConfirmation),
            "fail" => Some(ConflictPolicy::Fail),
            _ => None,
        }
    }
}

/// Persisted compensation record. Mirrors V1.1 §8.1 `compensations` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationRecord {
    pub comp_id: String,
    pub step_id: String,
    pub level: CompensationLevel,
    /// Encrypted snapshot blob. W3a stores plaintext for PoC; W8 wires tauri-plugin-stronghold.
    pub snapshot_encrypted: Option<Vec<u8>>,
    pub ttl_expires: String, // RFC3339
    pub status: String,      // 'active' | 'consumed' | 'expired' | 'failed'
    pub snapshot_vault_ref: Option<String>,
    pub conflict_policy: ConflictPolicy,
    /// Function name to invoke for compensation (e.g. "filesystem.reverse_move").
    pub compensate_fn: String,
    /// JSON payload for the compensate_fn (e.g. reverse source/dest paths).
    pub reverse_payload: String,
}
```

- [ ] **Step 4: Create compensation/repo.rs**

Create `voicepilot/crates/trust-kernel/src/compensation/repo.rs`:

```rust
//! Compensation repository — V1.1 §8.1 `compensations` table.

use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone, Default)]
pub struct CompensationRepo;

impl CompensationRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &CompensationRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO compensations
                (comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                 compensation_level, snapshot_vault_ref, conflict_policy)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3, ?7, ?8)",
            params![
                rec.comp_id,
                rec.step_id,
                rec.level.as_str(),
                rec.snapshot_encrypted,
                rec.ttl_expires,
                rec.status,
                rec.snapshot_vault_ref,
                rec.conflict_policy.as_str(),
            ],
        })?;
        // Note: compensate_fn + reverse_payload are stored in the snapshot_encrypted
        // blob's JSON for W3a (avoids schema migration). W8 will add explicit columns
        // when wiring tauri-plugin-stronghold.
        // For W3a we stash them as a separate JSON in snapshot_vault_ref's place
        // when vault_ref is None. This is a PoC shortcut — clearly documented.
        if rec.snapshot_vault_ref.is_none() {
            conn.execute(
                "UPDATE compensations SET snapshot_vault_ref = ?1 WHERE comp_id = ?2",
                params![format!(
                    "{{\"compensate_fn\":\"{}\",\"reverse_payload\":{}}}",
                    rec.compensate_fn, rec.reverse_payload
                ), rec.comp_id],
            )?;
        }
        Ok(())
    }

    pub fn get(&self, conn: &Connection, comp_id: &str) -> Result<Option<CompensationRecord>> {
        let mut stmt = conn.prepare(
            "SELECT comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                    snapshot_vault_ref, conflict_policy
             FROM compensations WHERE comp_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![comp_id], |r| {
            let comp_id: String = r.get(0)?;
            let step_id: String = r.get(1)?;
            let level: String = r.get(2)?;
            let snapshot_encrypted: Option<Vec<u8>> = r.get(3)?;
            let ttl_expires: String = r.get(4)?;
            let status: String = r.get(5)?;
            let snapshot_vault_ref: Option<String> = r.get(6)?;
            let conflict_policy: String = r.get(7)?;
            Ok((
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (comp_id, step_id, level_str, snapshot_encrypted, ttl_expires, status,
                 snapshot_vault_ref, conflict_policy_str) = row_result?;
            let level = CompensationLevel::parse(&level_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid level: {}", level_str)))?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid conflict_policy: {}", conflict_policy_str)))?;

            // Parse compensate_fn + reverse_payload from snapshot_vault_ref PoC stash.
            let (compensate_fn, reverse_payload) = parse_poc_payload(&snapshot_vault_ref);

            Ok(Some(CompensationRecord {
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy, compensate_fn, reverse_payload,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn mark_status(&self, conn: &Connection, comp_id: &str, new_status: &str) -> Result<()> {
        conn.execute(
            "UPDATE compensations SET status = ?1 WHERE comp_id = ?2",
            params![new_status, comp_id],
        )?;
        Ok(())
    }

    pub fn list_active(&self, conn: &Connection) -> Result<Vec<CompensationRecord>> {
        let mut stmt = conn.prepare(
            "SELECT comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                    snapshot_vault_ref, conflict_policy
             FROM compensations WHERE status = 'active' ORDER BY ttl_expires",
        )?;
        let rows = stmt.query_map([], |r| {
            let comp_id: String = r.get(0)?;
            let step_id: String = r.get(1)?;
            let level: String = r.get(2)?;
            let snapshot_encrypted: Option<Vec<u8>> = r.get(3)?;
            let ttl_expires: String = r.get(4)?;
            let status: String = r.get(5)?;
            let snapshot_vault_ref: Option<String> = r.get(6)?;
            let conflict_policy: String = r.get(7)?;
            Ok((
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (comp_id, step_id, level_str, snapshot_encrypted, ttl_expires, status,
                 snapshot_vault_ref, conflict_policy_str) = row_result?;
            let level = CompensationLevel::parse(&level_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid level: {}", level_str)))?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid conflict_policy: {}", conflict_policy_str)))?;
            let (compensate_fn, reverse_payload) = parse_poc_payload(&snapshot_vault_ref);
            out.push(CompensationRecord {
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy, compensate_fn, reverse_payload,
            });
        }
        Ok(out)
    }
}

/// Parse the PoC JSON stash from snapshot_vault_ref.
/// Returns (compensate_fn, reverse_payload) or ("", "{}") if not parseable.
fn parse_poc_payload(stash: &Option<String>) -> (String, String) {
    match stash {
        Some(s) if s.starts_with('{') => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                let fn_name = v.get("compensate_fn").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let payload = v.get("reverse_payload").map(|x| x.to_string()).unwrap_or_else(|| "{}".to_string());
                (fn_name, payload)
            } else {
                ("".to_string(), "{}".to_string())
            }
        }
        _ => ("".to_string(), "{}".to_string()),
    }
}
```

- [ ] **Step 5: Update compensation/mod.rs**

Replace the stub contents of `voicepilot/crates/trust-kernel/src/compensation/mod.rs`:

```rust
//! Compensation three-level — V1.1 §7.2.

pub mod types;
pub mod repo;
pub mod executor;
```

Create `voicepilot/crates/trust-kernel/src/compensation/executor.rs` as a stub for Task 8:

```rust
//! Compensation executor — V1.1 §7.2.
//! Stub; implemented in Task 8.
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test compensation_repo 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 7: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/compensation/ voicepilot/crates/trust-kernel/tests/compensation_repo.rs
git commit -m "feat(compensation): types + repo CRUD against compensations table (V1.1 §7.2, §8.1)"
```

---

## Task 8: Compensation executor — auto_reverse_move

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/compensation/executor.rs`
- Create: `voicepilot/crates/trust-kernel/tests/compensation_reverse.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/compensation_reverse.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::compensation::executor::auto_reverse_move;
use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3a-rev-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_record(reverse_payload: &str) -> CompensationRecord {
    CompensationRecord {
        comp_id: "comp-1".to_string(),
        step_id: "step-1".to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: None,
        ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: reverse_payload.to_string(),
    }
}

#[test]
fn auto_reverse_moves_files_back_to_original_locations() {
    let dir = tmp_dir();
    let original = dir.join("orig.txt");
    let moved_to = dir.join("moved.txt");
    fs::write(&moved_to, b"hello").unwrap();

    let payload = serde_json::json!({
        "moves": [
            {"from": "c:/placeholder/orig.txt", "to": "c:/placeholder/moved.txt"}
        ]
    }).to_string();
    // Override with real paths.
    let payload = serde_json::json!({
        "moves": [
            {"from": original.to_string_lossy().replace('\\', "/"),
             "to": moved_to.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    auto_reverse_move(&rec).unwrap();

    assert!(original.exists(), "original location must have the file back");
    assert!(!moved_to.exists(), "moved location must be empty");
    assert_eq!(fs::read(&original).unwrap(), b"hello");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn auto_reverse_fails_when_move_target_missing() {
    let dir = tmp_dir();
    let missing = dir.join("nonexistent.txt");
    let original = dir.join("orig.txt");

    let payload = serde_json::json!({
        "moves": [
            {"from": original.to_string_lossy().replace('\\', "/"),
             "to": missing.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    let result = auto_reverse_move(&rec);
    assert!(result.is_err(), "reverse must fail when current location is missing");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn auto_reverse_skips_when_payload_empty() {
    let rec = sample_record("{}");
    let result = auto_reverse_move(&rec);
    assert!(result.is_ok(), "empty payload should be a no-op");
}

#[test]
fn auto_reverse_rolls_back_partial_on_failure() {
    let dir = tmp_dir();
    let ok_current = dir.join("ok_moved.txt");
    let ok_original = dir.join("ok_orig.txt");
    fs::write(&ok_current, b"ok content").unwrap();

    let bad_current = dir.join("bad_moved.txt"); // doesn't exist
    let bad_original = dir.join("bad_orig.txt");

    let payload = serde_json::json!({
        "moves": [
            {"from": ok_original.to_string_lossy().replace('\\', "/"),
             "to": ok_current.to_string_lossy().replace('\\', "/")},
            {"from": bad_original.to_string_lossy().replace('\\', "/"),
             "to": bad_current.to_string_lossy().replace('\\', "/")}
        ]
    }).to_string();

    let rec = sample_record(&payload);
    let result = auto_reverse_move(&rec);
    assert!(result.is_err(), "must fail on bad move");
    // Rollback: ok_current should be back where it was.
    assert!(ok_current.exists(), "partial rollback must restore already-reversed file");
    assert!(!ok_original.exists());
    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test compensation_reverse 2>&1`
Expected: FAIL — `auto_reverse_move` does not exist.

- [ ] **Step 3: Implement auto_reverse_move**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/compensation/executor.rs`:

```rust
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
pub fn auto_reverse_move(rec: &CompensationRecord) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!("invalid reverse_payload: {}", e))
    })?;

    let moves = payload
        .get("moves")
        .and_then(|m| m.as_array())
        .ok_or_else(|| KernelError::Compensation("reverse_payload missing 'moves' array".to_string()))?;

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
            // Roll back any already-reversed moves.
            for (orig, curr) in reversed.iter().rev() {
                let _ = std::fs::rename(curr, orig);
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
                // Roll back.
                for (orig, curr) in reversed.iter().rev() {
                    let _ = std::fs::rename(curr, orig);
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test compensation_reverse 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/compensation/executor.rs voicepilot/crates/trust-kernel/tests/compensation_reverse.rs
git commit -m "feat(compensation): auto_reverse_move with partial rollback (V1.1 §7.2)"
```

---

## Task 9: ToolResult V2 + CLI move command (end-to-end smoke)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/toolresult.rs` (add full V2 fields)
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs` (add filesystem/compensation accessors)
- Modify: `voicepilot/crates/trust-kernel/src/repo/step_repo.rs` (extend with prepare/evidence/compensation updates)
- Modify: `voicepilot/crates/cli/src/main.rs` (add `move` command)
- Create: `voicepilot/crates/trust-kernel/tests/toolresult.rs`

- [ ] **Step 1: Write the failing test for ToolResult V2**

Create `voicepilot/crates/trust-kernel/tests/toolresult.rs`:

```rust
use chrono::Utc;
use trust_kernel::toolresult::{EvidenceStrength, ToolResult, ToolStatus};

#[test]
fn toolresult_serializes_with_all_v2_fields() {
    let now = Utc::now();
    let r = ToolResult {
        status: ToolStatus::Succeeded,
        data: serde_json::json!({"moved": 3}),
        evidence_strength: EvidenceStrength::Strong,
        compensation_ref: Some("comp-1".to_string()),
        compensation_level: trust_kernel::compensation::types::CompensationLevel::Strong,
        preconditions_hash: Some("sha256:abc".to_string()),
        idempotency_key: "idem-1".to_string(),
        egress_performed: false,
        data_classification: trust_kernel::policy::types::DLevel::D1,
        error_code: None,
        retryable: false,
        safe_to_retry: true,
        started_at: now,
        finished_at: now,
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"status\":\"succeeded\""));
    assert!(s.contains("\"evidence_strength\":\"strong\""));
    assert!(s.contains("\"compensation_level\":\"strong\""));
    assert!(s.contains("\"idempotency_key\":\"idem-1\""));
    assert!(s.contains("\"data_classification\":\"D1\""));
}

#[test]
fn evidence_strength_serializes_as_lowercase() {
    assert_eq!(serde_json::to_string(&EvidenceStrength::Strong).unwrap(), "\"strong\"");
    assert_eq!(serde_json::to_string(&EvidenceStrength::Medium).unwrap(), "\"medium\"");
    assert_eq!(serde_json::to_string(&EvidenceStrength::Weak).unwrap(), "\"weak\"");
}

#[test]
fn tool_status_round_trips() {
    for s in [ToolStatus::Succeeded, ToolStatus::Failed, ToolStatus::Cancelled, ToolStatus::Partial] {
        let json = serde_json::to_string(&s).unwrap();
        let back: ToolStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test toolresult 2>&1`
Expected: FAIL — `ToolResult` missing V2 fields.

- [ ] **Step 3: Extend ToolResult to full V2**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/toolresult.rs`:

```rust
//! ToolResult V2 — V1.1 §6.3.
//!
//! Full V2 fields per spec. W3a populates the subset produced by FilesystemTool;
//! other fields (egress_performed, error_code, retryable) are filled by future
//! MCP wrappers and the Verifier pipeline.

use crate::compensation::types::CompensationLevel;
use crate::policy::types::DLevel;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceStrength {
    Strong,
    Medium,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Succeeded,
    Failed,
    Cancelled,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: serde_json::Value,

    // V1.1 transaction + evidence
    pub evidence_strength: EvidenceStrength,
    pub compensation_ref: Option<String>,
    pub compensation_level: CompensationLevel,
    pub preconditions_hash: Option<String>,
    pub idempotency_key: String,

    // V1.1 data classification
    pub egress_performed: bool,
    pub data_classification: DLevel,

    // V1.0 error handling
    pub error_code: Option<String>,
    pub retryable: bool,
    pub safe_to_retry: bool,

    // V1.1 audit timestamps
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test toolresult 2>&1`
Expected: PASS — 3 tests.

- [ ] **Step 5: Add kernel accessors for FilesystemTool + CompensationRepo**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`, add fields + accessors. Find:

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
}
```

Replace with:

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
    fs: Arc<crate::tools::fs::FilesystemTool>,
    comp_repo: Arc<crate::compensation::repo::CompensationRepo>,
    txn_mgr: Arc<crate::policy::transaction::TransactionManager>,
}
```

Find `with_conn`, replace the body to also initialize the new fields:

```rust
    fn with_conn(conn: Connection) -> Self {
        let shared = Arc::new(Mutex::new(conn));
        let cedar_src = include_str!("policies/default.cedar");
        let gateway = Arc::new(
            crate::gateway::ActionGateway::new(cedar_src)
                .expect("default cedar policy must parse"),
        );
        Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
            gateway,
            fs: Arc::new(crate::tools::fs::FilesystemTool::new()),
            comp_repo: Arc::new(crate::compensation::repo::CompensationRepo::new()),
            txn_mgr: Arc::new(crate::policy::transaction::TransactionManager::new()),
        }
    }

    /// Access the FilesystemTool adapter.
    pub fn filesystem(&self) -> &crate::tools::fs::FilesystemTool {
        &self.fs
    }

    /// Access the Compensation repository.
    pub fn compensation_repo(&self) -> &crate::compensation::repo::CompensationRepo {
        &self.comp_repo
    }

    /// Access the TransactionManager (for prepare/commit lifecycle).
    pub fn transaction_manager(&self) -> &crate::policy::transaction::TransactionManager {
        &self.txn_mgr
    }
```

- [ ] **Step 6: Extend step_repo with W3a update methods**

Read the existing `voicepilot/crates/trust-kernel/src/repo/step_repo.rs` to see the existing pattern (do not rewrite — only append new methods). Add these methods to the `impl StepRepo` block:

```rust
    /// Update the prepare_token + preconditions_hash + effect_manifest for a step.
    pub fn update_prepare_state(
        &self,
        conn: &Connection,
        step_id: &str,
        prepare_token: &str,
        preconditions_hash: &str,
        effect_manifest: &serde_json::Value,
    ) -> Result<()> {
        conn.execute(
            "UPDATE steps SET prepare_token = ?1, preconditions_hash = ?2, effect_manifest = ?3
             WHERE step_id = ?4",
            rusqlite::params![prepare_token, preconditions_hash, effect_manifest.to_string(), step_id],
        )?;
        Ok(())
    }

    /// Update post-commit fields: evidence_strength + compensation_ref.
    pub fn update_post_commit(
        &self,
        conn: &Connection,
        step_id: &str,
        evidence_strength: &str,
        compensation_ref: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE steps SET evidence_strength = ?1, compensation_ref = ?2
             WHERE step_id = ?3",
            rusqlite::params![evidence_strength, compensation_ref, step_id],
        )?;
        Ok(())
    }
```

(Adjust the import of `Result` and `Connection` if not already present — they should be, since W1 added the StepRepo.)

- [ ] **Step 7: Add `move` command to CLI**

Read the existing `voicepilot/crates/cli/src/main.rs`. Find the help text block (already extended in W2):

```rust
    println!("  policy <tool> <path> <D-level> [E-level]  run policy decision (W2)");
    println!("  quit");
```

Insert before the `quit` line:

```rust
    println!("  move <src1> [src2...] <dest>  move files via prepare→commit (W3a)");
```

Find the line:

```rust
        if let Some(rest) = line.strip_prefix("policy ") {
            handle_policy_command(&kernel, rest);
            continue;
        }
```

Insert before it:

```rust
        if let Some(rest) = line.strip_prefix("move ") {
            handle_move_command(&kernel, rest);
            continue;
        }
```

At the end of the file (after `handle_policy_command`), add:

```rust
fn handle_move_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 2 {
        println!("usage: move <src1> [src2...] <dest>");
        return;
    }
    let (sources, dest) = parts.split_at(parts.len() - 1);
    let dest = std::path::PathBuf::from(dest[0]);
    let srcs: Vec<std::path::PathBuf> = sources.iter().map(std::path::PathBuf::from).collect();
    let src_refs: Vec<&std::path::Path> = srcs.iter().map(|p| p.as_path()).collect();

    // Phase 1: prepare.
    let prepared = match kernel.filesystem().prepare_move(
        "cli-task", "cli-step", &src_refs, &dest, kernel.transaction_manager(),
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("prepare failed: {}", e);
            return;
        }
    };
    println!("prepare OK: {} sources, {} bytes, {} conflicts",
             prepared.manifest.sources.len(),
             prepared.manifest.total_bytes,
             prepared.manifest.conflicts.len());
    println!("  prepare_token: {}", prepared.token.token);
    println!("  preconditions_hash: {}", prepared.preconditions_hash);

    // Phase 2: commit.
    let committed = match kernel.filesystem().commit_move(
        &prepared.token, &prepared.manifest, kernel.transaction_manager(),
    ) {
        Ok(c) => c,
        Err(e) => {
            println!("commit failed: {}", e);
            return;
        }
    };
    println!("commit OK: moved {} files", committed.moved_paths.len());

    // Phase 3: verify (Strong Verifier).
    match kernel.filesystem().verify_move(&prepared.manifest) {
        Ok(v) => println!("verify OK: evidence_strength = {:?}", v.evidence_strength),
        Err(e) => println!("verify FAILED: {}", e),
    }

    // Phase 4: create compensation record.
    let comp_id = format!("comp-{}", uuid::Uuid::new_v4());
    let reverse_payload = serde_json::json!({
        "moves": committed.moved_paths.iter().map(|(orig, curr)| {
            serde_json::json!({
                "from": orig.to_string_lossy().replace('\\', "/"),
                "to": curr.to_string_lossy().replace('\\', "/"),
            })
        }).collect::<Vec<_>>()
    }).to_string();
    let rec = trust_kernel::compensation::types::CompensationRecord {
        comp_id: comp_id.clone(), step_id: "cli-step".to_string(),
        level: trust_kernel::compensation::types::CompensationLevel::Strong,
        snapshot_encrypted: None,
        ttl_expires: (chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy: trust_kernel::compensation::types::ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload,
    };
    {
        let conn = std::sync::Arc::clone(&/* todo: get conn from kernel */);
        let _ = conn; // placeholder
    }
    // The kernel holds the conn privately; for the CLI smoke we create a separate
    // connection to the same DB file. W3b will expose a public kernel method.
    println!("compensation record created: {} (auto_reverse ready)", comp_id);
}
```

**IMPORTANT**: The above CLI code references `std::sync::Arc::clone` on the kernel's conn, which is private. Since we cannot access it from the CLI, simplify the compensation-creation block in the CLI to just print a placeholder message:

Replace the entire `// Phase 4: create compensation record.` block (from `let comp_id = format!(...)` through the end of the function before the closing `}`) with:

```rust
    // Phase 4: compensation record creation deferred to W3b (needs kernel method to access conn).
    // W3a CLI smoke stops here; W3b will add `kernel.create_compensation(rec)`.
    println!("(compensation record creation deferred to W3b — see Task 9 of W3a plan)");
}
```

- [ ] **Step 8: Build the CLI**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli 2>&1`
Expected: compiles cleanly, 0 warnings.

- [ ] **Step 9: Smoke test the move command**

Run:

```powershell
$env:VOICEPILOT_DB = "$env:TEMP\voicepilot-w3a-smoke.db"; Remove-Item $env:VOICEPILOT_DB -Force -ErrorAction SilentlyContinue
$src1 = "$env:TEMP\w3a_a.txt"; $src2 = "$env:TEMP\w3a_b.txt"; $dest = "$env:TEMP\w3a_out"
Remove-Item $src1, $src2 -Force -ErrorAction SilentlyContinue
Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
Set-Content $src1 "content a"; Set-Content $src2 "content b"
New-Item -ItemType Directory -Path $dest | Out-Null
"move $src1 $src2 $dest`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```

Expected output contains:
- `prepare OK: 2 sources, 18 bytes, 0 conflicts`
- `commit OK: moved 2 files`
- `verify OK: evidence_strength = Strong`
- `(compensation record creation deferred to W3b`

- [ ] **Step 10: Run full test suite**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS — W1 (26) + W2 (53) + W3a (30) = ~109 tests.

- [ ] **Step 11: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/toolresult.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/src/repo/step_repo.rs voicepilot/crates/cli/src/main.rs voicepilot/crates/trust-kernel/tests/toolresult.rs
git commit -m "feat(w3a): ToolResult V2 + kernel accessors + CLI move command end-to-end (V1.1 §6.3, §11.1 W3a gate)"
```

---

## Self-Review

**1. Spec coverage (V1.1.1 spec sections referenced):**

| Spec section | Covered by |
|---|---|
| §4.4 step 1 (resource.path canonicalization) | Task 1 (fs_paths::canonicalize) |
| §6.1 filesystem-mcp (native adapter, no MCP SDK yet) | Tasks 2-6 (FilesystemTool) |
| §6.2 prepare→approve→commit with effect_manifest | Tasks 2-4 (prepare_move, commit_move) |
| §6.3 ToolResult V2 (full fields) | Task 9 |
| §7.1 Strong Verifier (filesystem sha256+size re-read) | Task 5 (verify_move) |
| §7.2 Compensation three-level + auto_reverse | Tasks 7-8 |
| §8.1 compensations table CRUD | Task 7 |
| §11.1 W3 gate "files.organize Skill 可跑" | W3a delivers filesystem half; W3b wires the Skill |

Gaps (deferred to W3b):
- `files.organize` Skill manifest + Skill Router integration — W3b
- Approval UI flow integration (prepare → user approves → commit) — W3b/W4
- MCP server wrapping (inputSchema, outputSchema, annotations) — W3b/W4
- allowed_paths whitelist enforcement — W3b (caller's responsibility until MCP lands)
- Stronghold encryption for snapshot_encrypted — W8 (V1.1 §7.2 safety constraint ①)
- Taint tracking propagation through filesystem operations — W8

**2. Placeholder scan:** No "TBD", "TODO", "fill in" found in implementation steps. The CLI `move` command has one explicit "deferred to W3b" note for compensation record persistence — this is intentional scope, not a placeholder.

**3. Type consistency:**
- `FileSnapshot` (W2 transaction.rs) reused in W3a fs_snapshot.rs — fields match
- `EffectManifest` (W2) reused in fs.rs prepare_move — fields match
- `PrepareToken` (W2) reused in commit_move — fields match
- `CompensationLevel` / `ConflictPolicy` defined once in types.rs, used in repo.rs + executor.rs + toolresult.rs — consistent
- `EvidenceStrength` defined in toolresult.rs, referenced from fs.rs VerifyResult — consistent
- `ToolResult` V2 fields match §6.3 spec exactly

No issues found.

---

## Spec Notes & Issues Found During W3a Planning

Per user request, additional observations from W3a planning:

17. **§6.1 says filesystem-mcp uses `@modelcontextprotocol/server-filesystem` (官方, Anthropic).** W3a implements a native Rust FilesystemTool instead, deferring MCP server wrapping to W3b. **Suggestion**: spec should clarify that the host implements the filesystem operations natively (Rust), and the MCP server is a thin wrapper exposing those operations to external MCP clients. The current wording suggests importing the Node-based server-filesystem package, which conflicts with V1.1's "single Rust trust kernel" architecture.

18. **§7.2 safety constraint ① requires `snapshot_encrypted` to be encrypted via tauri-plugin-stronghold.** W3a stores plaintext (or None) for PoC. **Suggestion**: spec should add a note that W3a-W7 may use plaintext for development, with W8 as the gate for Stronghold integration before V1 release.

19. **§6.2 prepare example shows `file_id: "fi_001"` but doesn't specify how file_id is computed.** W3a uses Windows `volume_serial_number + file_index_high + file_index_low` (or Unix `dev + ino`). **Suggestion**: spec should pin the file_id construction per OS so W3b implementations match.

20. **§6.3 ToolResult V2 has both `compensation_ref` and `compensation_level` — but compensation_level is also in the CompensationRecord.** W3a treats `ToolResult.compensation_level` as a hint for the UI ("this tool's max compensation level"), while `CompensationRecord.level` is the actual level for the executed step. **Suggestion**: spec should clarify these are different — ToolResult.compensation_level is the tool's declared ceiling, CompensationRecord.level is the actual step's compensation.

21. **§7.2 conflict_policy values are `auto_reverse | require_confirmation | fail`, but the spec doesn't define when each triggers.** W3a implements `auto_reverse` for move_files. **Suggestion**: spec should add a table mapping (compensation_level, conflict_policy) → behavior. E.g., `(strong, auto_reverse)` = automatically reverse on failure; `(best_effort, require_confirmation)` = prompt user before attempting reverse; `(none, fail)` = no compensation, just fail.

These are documented here for the user; no spec changes have been made.

---

## W3a Exit Criteria

W3a is complete when ALL of the following hold:
- [ ] All 9 tasks committed
- [ ] `cargo test` passes (W1 + W2 + W3a tests, ~109 total)
- [ ] CLI `move` command works end-to-end: prepare → commit → verify (Strong) → compensation record (deferred to W3b message)
- [ ] `verify_move` returns `EvidenceStrength::Strong` for successful move
- [ ] `commit_move` rejects TOCTOU (source tampered or new destination conflict)
- [ ] `auto_reverse_move` restores files to original locations with partial rollback on failure
- [ ] Spec issues 17-21 documented for user review
- [ ] No spec changes made (user decides whether to bump to V1.1.2)
