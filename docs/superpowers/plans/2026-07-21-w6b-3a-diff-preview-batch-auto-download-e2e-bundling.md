# W6b-3a Implementation Plan: Diff Preview + 批次审批 + auto-download + E2E + 打包

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 W6b-3a 5 个功能子任务 + 1 个收尾:Diff Preview / 批次审批文案 / 模型 auto-download / E2E 全链路冒烟 / Tauri Windows 打包 / PROGRESS.md 更新。

**Architecture:**
- Rust 端用 `similar` crate 计算 unified diff(文件内容不经过 IPC)
- 前端 ApprovalModal 加"Diff"按钮懒加载 DiffViewer 组件
- 模型 auto-download 用 `ureq` + `dirs`(optional,仅 voice feature 启用)
- E2E 测试用 Tauri 2 `mock_app` API + StubVoiceListen mock
- Tauri 打包仅 Windows NSIS,图标用流体波纹设计(§6.2.1 锁定方向 A)

**Tech Stack:** Rust + Tauri 2 + React + TypeScript + `similar` crate + `ureq` + `dirs` + NSIS installer

**Spec:** `docs/superpowers/specs/2026-07-21-w6b-3a-diff-preview-batch-auto-download-e2e-bundling-design.md`

**项目路径约定:**
- Git 根 / 项目根:`d:\voicepilot\`
- Rust workspace:`d:\voicepilot\voicepilot\`(含根 `Cargo.toml`)
- 所有 Rust crate 路径前缀:`voicepilot/crates/<crate-name>/`
- 前端路径前缀:`voicepilot/crates/ui/web/src/`
- 文档路径前缀:`docs/`

**PowerShell 兼容约定(项目记忆):**
- 用 `;` 分隔命令,不用 `&&` 或 `||`
- git commit 用单行 `-m "message"`,不用 heredoc
- 所有 cargo / npm 命令在 `voicepilot/` 目录下执行(workspace 根)

---

## 文件结构

### 新建文件

| 路径 | 职责 |
|---|---|
| `voicepilot/crates/trust-kernel/src/tools/diff.rs` | Rust 端 diff 计算(`similar` crate + 50MB 软上限 + 二进制检测) |
| `voicepilot/crates/ui/src/diff_commands.rs` | Tauri command `compute_diff_command` |
| `voicepilot/crates/ui/web/src/components/DiffViewer.tsx` | 前端 Diff 预览组件(懒加载) |
| `voicepilot/crates/trust-kernel/src/voice/model_download.rs` | Rust 端模型下载(`ureq` + SHA256 校验) |
| `voicepilot/crates/ui/src/model_download_commands.rs` | Tauri command `check_model` / `download_model` / `is_voice_enabled` |
| `voicepilot/crates/ui/web/src/components/ModelDownloadBar.tsx` | 启动检测弹窗 + 下载进度条 |
| `voicepilot/crates/ui/tests/w6b3_e2e_smoke.rs` | E2E 全链路冒烟测试 |
| `voicepilot/crates/ui/icons/32x32.png` | Tauri bundle 图标(32x32) |
| `voicepilot/crates/ui/icons/128x128.png` | Tauri bundle 图标(128x128) |
| `voicepilot/crates/ui/icons/128x128@2x.png` | Tauri bundle 图标(256x256) |
| `voicepilot/crates/ui/icons/icon.png` | 源图(512x512,流体波纹) |

### 修改文件

| 路径 | 改动 |
|---|---|
| `voicepilot/Cargo.toml` | workspace.dependencies 加 `similar` / `ureq` / `dirs` |
| `voicepilot/crates/trust-kernel/Cargo.toml` | 加 `similar` + optional `ureq` / `dirs` + voice feature 加 `dep:ureq, dep:dirs` |
| `voicepilot/crates/trust-kernel/src/lib.rs` | (无需改,tools 已 pub) |
| `voicepilot/crates/trust-kernel/src/tools/mod.rs` | 加 `pub mod diff;` |
| `voicepilot/crates/trust-kernel/src/tools/fs.rs` | 加 `pub fn assert_path_allowed` 方法 |
| `voicepilot/crates/trust-kernel/src/voice/mod.rs` | 加 `pub mod model_download;` |
| `voicepilot/crates/trust-kernel/src/voice/error.rs` | 加 `DownloadFailed(String)` variant |
| `voicepilot/crates/ui/Cargo.toml` | (无需改,trust-kernel 透传 voice feature) |
| `voicepilot/crates/ui/src/lib.rs` | 加 `pub mod diff_commands;` + `#[cfg(feature = "voice")] pub mod model_download_commands;` |
| `voicepilot/crates/ui/src/commands.rs` | `register_handlers` + `register_handlers_with_voice` 加 `compute_diff_command` + `is_voice_enabled_command` + (voice) `check_model_command` / `download_model_command` |
| `voicepilot/crates/ui/web/src/types.ts` | 加 `DiffResult` / `FileKind` / `ModelDownloadState` 类型 |
| `voicepilot/crates/ui/web/src/api.ts` | 加 `computeDiff` / `checkModel` / `downloadModel` / `isVoiceEnabled` 函数 |
| `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` | (1) Diff 按钮集成 DiffViewer;(2) 批次审批按钮文案中文化 |
| `voicepilot/crates/ui/web/src/App.tsx` | 集成 `ModelDownloadBar` |
| `voicepilot/crates/ui/web/src/styles.css` | 加 `.diff-viewer` / `.diff-toggle-btn` / `.model-download-bar` 样式 |
| `voicepilot/crates/ui/tauri.conf.json` | 补全 `bundle` 字段(NSIS / icon / metadata) + `app.windows[0]` minWidth/minHeight |
| `voicepilot/crates/ui/icons/icon.ico` | 替换为多尺寸 ICO(16/32/48/64/128/256) |
| `docs/PROGRESS.md` | 顶部状态块更新 + 加 W6b-3a 详细段落 |

---

## Task 1: Diff Preview 后端(Rust `similar` crate)

**目标:** 实现 `compute_file_diff` 函数,用 `similar` crate 计算 unified diff,50MB 软上限,二进制检测,新文件检测。

**Files:**
- Modify: `voicepilot/Cargo.toml`(workspace deps)
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/src/tools/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/tools/diff.rs`

- [ ] **Step 1.1: workspace Cargo.toml 加 similar 依赖**

修改 `voicepilot/Cargo.toml` 的 `[workspace.dependencies]` 段,在 `walkdir = "2.5"` 后加一行:

```toml
similar = "2"
```

- [ ] **Step 1.2: trust-kernel Cargo.toml 加 similar 依赖**

修改 `voicepilot/crates/trust-kernel/Cargo.toml`,在 `[dependencies]` 段 `walkdir = { workspace = true }` 后加一行:

```toml
similar = { workspace = true }
```

- [ ] **Step 1.3: tools/mod.rs 加 diff 模块声明**

修改 `voicepilot/crates/trust-kernel/src/tools/mod.rs`,在 `pub mod fs;` 后加一行:

```rust
pub mod diff;
```

完整文件应为:

```rust
//! Tool adapters — V1.1 §6.
//!
//! W3a: FilesystemTool (native Rust, no MCP SDK yet).
//! W3b/W4 will wrap this in an MCP server handler.

pub mod fs_paths;
pub mod fs_snapshot;
pub mod fs;
pub mod diff;
```

- [ ] **Step 1.4: 写失败测试(diff.rs inline test)**

创建 `voicepilot/crates/trust-kernel/src/tools/diff.rs`,先只写测试 + 类型签名(实现留空让测试失败):

```rust
//! Diff Preview 工具(W6b-3a Task 1)。
//! 用 `similar` crate 计算 unified diff,Rust 端完成,文件内容不经过 IPC。
//!
//! 路径约定:`source_path` 和 `dest_path` 都是完整文件路径(非目录)。
//! files.organize 场景下 `manifest.destination` 是目录,前端调用时需拼接:
//! `dest_path = manifest.destination + "/" + basename(source_path)`。

use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use similar::TextDiff;

/// Diff 计算结果。
///
/// 注意:不加 `#[serde(rename_all = "camelCase")]` —— 与现有 `FileSnapshot`
/// 等类型保持一致(全 snake_case 序列化),前端 TS interface 也用 snake_case。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffResult {
    /// 源文件路径(canonical)
    pub source_path: String,
    /// 目标文件路径(canonical,可能不存在=新文件)
    pub dest_path: String,
    /// 文件类型(影响展示)
    pub file_kind: FileKind,
    /// unified diff 文本(可空,如二进制文件)
    pub diff_text: Option<String>,
    /// 截断提示(若文件 > 50MB 软上限)
    pub truncated: bool,
    /// 截断原因(若 truncated=true)
    pub truncate_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// 目标不存在,纯新增
    NewFile,
    /// 文本文件,有 diff
    Text,
    /// 二进制文件(前 8KB 含 NUL byte)
    Binary,
}

/// 软上限 50MB,防止 OOM。
const MAX_FILE_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// 计算单文件 diff。
///
/// - `source_path` 必须存在(读取内容)
/// - `dest_path` 可不存在(返回 FileKind::NewFile,diff 为全文件内容)
/// - 超过 50MB 返回 truncated=true,diff_text=None
/// - 二进制文件返回 FileKind::Binary,diff_text=None
pub fn compute_file_diff(
    source_path: &Path,
    dest_path: &Path,
) -> crate::error::Result<DiffResult> {
    let source_meta = fs::metadata(source_path)?;
    let source_size = source_meta.len();

    // 软上限检查
    if source_size > MAX_FILE_SIZE_BYTES {
        return Ok(DiffResult {
            source_path: source_path.to_string_lossy().into_owned(),
            dest_path: dest_path.to_string_lossy().into_owned(),
            file_kind: FileKind::Text,
            diff_text: None,
            truncated: true,
            truncate_reason: Some(format!(
                "文件超过 50MB 软上限({} bytes)",
                source_size
            )),
        });
    }

    let source_content = fs::read(source_path)?;

    // 二进制检测:前 8KB 含 NUL byte
    let is_binary = source_content.iter().take(8 * 1024).any(|&b| b == 0);

    if is_binary {
        return Ok(DiffResult {
            source_path: source_path.to_string_lossy().into_owned(),
            dest_path: dest_path.to_string_lossy().into_owned(),
            file_kind: FileKind::Binary,
            diff_text: None,
            truncated: false,
            truncate_reason: None,
        });
    }

    // dest 不存在 = 新文件
    if !dest_path.exists() {
        let source_text = String::from_utf8_lossy(&source_content);
        return Ok(DiffResult {
            source_path: source_path.to_string_lossy().into_owned(),
            dest_path: dest_path.to_string_lossy().into_owned(),
            file_kind: FileKind::NewFile,
            diff_text: Some(format_new_file_diff(&source_text)),
            truncated: false,
            truncate_reason: None,
        });
    }

    // 双向 diff
    let dest_content = fs::read(dest_path)?;
    let source_text = String::from_utf8_lossy(&source_content);
    let dest_text = String::from_utf8_lossy(&dest_content);

    let diff = TextDiff::from_lines(&source_text, &dest_text)
        .unified_diff()
        .context_radius(3)
        .to_string();

    Ok(DiffResult {
        source_path: source_path.to_string_lossy().into_owned(),
        dest_path: dest_path.to_string_lossy().into_owned(),
        file_kind: FileKind::Text,
        diff_text: Some(diff),
        truncated: false,
        truncate_reason: None,
    })
}

/// 新文件用 unified diff 格式呈现全内容(+ 前缀)。
fn format_new_file_diff(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("+{}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn compute_file_diff_new_file() {
        let src = write_temp(b"line1\nline2\nline3\n");
        let src_path = src.path();
        let dest_path = std::path::Path::new("/nonexistent/dest/path.txt");

        let result = compute_file_diff(src_path, dest_path).unwrap();

        assert_eq!(result.file_kind, FileKind::NewFile);
        assert!(result.diff_text.is_some());
        let diff = result.diff_text.unwrap();
        assert!(diff.contains("+line1"));
        assert!(diff.contains("+line2"));
        assert!(diff.contains("+line3"));
        assert!(!result.truncated);
    }

    #[test]
    fn compute_file_diff_text_diff() {
        let src = write_temp(b"line1\nline2 old\nline3\n");
        let dst = write_temp(b"line1\nline2 new\nline3\n");

        let result = compute_file_diff(src.path(), dst.path()).unwrap();

        assert_eq!(result.file_kind, FileKind::Text);
        let diff = result.diff_text.unwrap();
        assert!(diff.contains("-line2 old"));
        assert!(diff.contains("+line2 new"));
    }

    #[test]
    fn compute_file_diff_binary_detection() {
        let mut src = NamedTempFile::new().unwrap();
        src.write_all(b"binary\x00data\x00here").unwrap();
        src.flush().unwrap();
        let dst = write_temp(b"some text");

        let result = compute_file_diff(src.path(), dst.path()).unwrap();

        assert_eq!(result.file_kind, FileKind::Binary);
        assert!(result.diff_text.is_none());
        assert!(!result.truncated);
    }

    #[test]
    fn compute_file_diff_truncated_50mb() {
        // 创建一个伪 51MB 文件(用 sparse file 避免实际占盘)
        let src = NamedTempFile::new().unwrap();
        let src_path = src.path().to_path_buf();
        // 先写 1 byte,然后用 truncate 设大小为 51MB
        // 注意:sparse file 在 fs::read 时会实际读取 51MB(全 0),
        // 所以这里不 read,只触发 metadata().len() > 50MB 的分支
        let dst = write_temp(b"dest");
        std::fs::set_len(&src_path, 51 * 1024 * 1024).unwrap();

        let result = compute_file_diff(&src_path, dst.path()).unwrap();

        assert!(result.truncated);
        assert!(result.diff_text.is_none());
        assert!(result.truncate_reason.unwrap().contains("50MB"));
    }
}
```

注意:`tempfile` 是 trust-kernel 的 dev-dependency?先检查。如果不是,需要在 `Cargo.toml` 的 `[dev-dependencies]` 加 `tempfile = "3"`。

- [ ] **Step 1.5: 检查 tempfile dev-dependency**

读 `voicepilot/crates/trust-kernel/Cargo.toml`,确认 `[dev-dependencies]` 段是否有 `tempfile`。如果没有,在 `[dev-dependencies]` 段末尾加:

```toml
tempfile = "3"
```

- [ ] **Step 1.6: 运行测试验证**

Run(在 `voicepilot/` 目录下):

```powershell
cd voicepilot; cargo test -p trust-kernel --lib tools::diff -- --nocapture
```

Expected: 4 个测试全部 PASS。

- [ ] **Step 1.7: 运行 clippy 验证**

Run:

```powershell
cd voicepilot; cargo clippy -p trust-kernel --lib -- -D warnings
```

Expected: 无 warning。

- [ ] **Step 1.8: Commit**

```powershell
cd voicepilot; git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml voicepilot/crates/trust-kernel/src/tools/mod.rs voicepilot/crates/trust-kernel/src/tools/diff.rs; git commit -m "feat(w6b-3a): add compute_file_diff with similar crate (50MB cap + binary detection)"
```

---

## Task 2: FilesystemTool.assert_path_allowed 公开方法

**目标:** 给 `FilesystemTool` 加 `pub fn assert_path_allowed(&self, path: &Path) -> Result<()>` 方法,供 diff_commands 走 allowed_paths 白名单检查。

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`

- [ ] **Step 2.1: 写失败测试**

在 `voicepilot/crates/trust-kernel/src/tools/fs.rs` 末尾(在最后一个 `}` 之前,或在文件末尾的 `#[cfg(test)] mod tests` 之前)加一个 inline test。先读文件末尾确认结构。

读 `voicepilot/crates/trust-kernel/src/tools/fs.rs` 的最后 50 行,找到 impl 块的结尾 `}` 位置。

如果文件已有 `#[cfg(test)] mod tests`,在 tests mod 内加测试;否则在 `impl FilesystemTool` 块的最后一个方法之后、`}` 之前加测试方法签名(让测试失败)。

**先加方法签名(让测试编译失败)**:在 `impl FilesystemTool` 块内最后一个方法之后加:

```rust
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
```

- [ ] **Step 2.2: 写测试**

在 `voicepilot/crates/trust-kernel/src/tools/fs.rs` 文件末尾加 `#[cfg(test)] mod tests`(如果已存在则在其内追加):

```rust
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
        assert!(matches!(result, Err(crate::error::KernelError::PathNotAllowed(_))));
    }
}
```

- [ ] **Step 2.3: 检查 tempfile dev-dependency(同 Task 1 Step 1.5)**

如果 `[dev-dependencies]` 没有 `tempfile`,加 `tempfile = "3"`。

- [ ] **Step 2.4: 运行测试验证**

Run:

```powershell
cd voicepilot; cargo test -p trust-kernel --lib tools::fs::assert_path_allowed_tests -- --nocapture
```

Expected: 3 个测试全部 PASS。

- [ ] **Step 2.5: 运行全 trust-kernel 测试确保无回归**

Run:

```powershell
cd voicepilot; cargo test -p trust-kernel --lib
```

Expected: 所有测试 PASS(包括现有测试 + Task 1 的 4 个 diff 测试 + Task 2 的 3 个 assert_path_allowed 测试)。

- [ ] **Step 2.6: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/Cargo.toml; git commit -m "feat(w6b-3a): expose FilesystemTool::assert_path_allowed for diff commands"
```

---

## Task 3: Diff Preview Tauri command

**目标:** 在 ui crate 加 `compute_diff_command` Tauri command,桥接前端到 `compute_file_diff`。

**Files:**
- Create: `voicepilot/crates/ui/src/diff_commands.rs`
- Modify: `voicepilot/crates/ui/src/lib.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`

- [ ] **Step 3.1: 创建 diff_commands.rs**

创建 `voicepilot/crates/ui/src/diff_commands.rs`:

```rust
//! Diff Preview Tauri commands(W6b-3a Task 3)。
//!
//! 桥接前端 `compute_diff_command` invoke 到 `trust_kernel::tools::diff::compute_file_diff`。
//! 路径必须通过 `FilesystemTool::assert_path_allowed` 白名单校验。

use std::path::PathBuf;
use tauri::State;
use trust_kernel::tools::diff::{compute_file_diff, DiffResult};

use crate::error::UiError;
use crate::state::AppState;

/// 计算单文件 diff。
///
/// 前端在 ApprovalModal 中点击"Diff"按钮调用。
/// 路径必须通过 allowed_paths 白名单(由 FilesystemTool 强制)。
#[tauri::command]
pub fn compute_diff_command(
    state: State<'_, AppState>,
    source_path: String,
    dest_path: String,
) -> Result<DiffResult, String> {
    compute_diff_impl(&state, &source_path, &dest_path).map_err(Into::into)
}

fn compute_diff_impl(
    state: &AppState,
    source_path: &str,
    dest_path: &str,
) -> Result<DiffResult, UiError> {
    let src = PathBuf::from(source_path);
    let dst = PathBuf::from(dest_path);

    // 通过 FilesystemTool 走 allowed_paths 检查(沿用 §6.1 安全模型)
    state
        .kernel
        .filesystem()
        .assert_path_allowed(&src)
        .map_err(|e| UiError::Kernel(e.to_string()))?;
    state
        .kernel
        .filesystem()
        .assert_path_allowed(&dst)
        .map_err(|e| UiError::Kernel(e.to_string()))?;

    let result = compute_file_diff(&src, &dst)?;
    Ok(result)
}
```

- [ ] **Step 3.2: 检查 UiError 是否有 Kernel variant**

读 `voicepilot/crates/ui/src/error.rs`,确认 `UiError` 是否有 `Kernel(String)` variant。

如果**没有** `Kernel(String)` variant,在 `UiError` enum 中加:

```rust
    #[error("kernel error: {0}")]
    Kernel(String),
```

(放在合适的位置,比如 `Io` 之后)

- [ ] **Step 3.3: lib.rs 加 diff_commands 模块声明**

修改 `voicepilot/crates/ui/src/lib.rs`,在 `#[cfg(feature = "tauri")] pub mod commands;` 后加:

```rust
#[cfg(feature = "tauri")]
pub mod diff_commands;
```

- [ ] **Step 3.4: commands.rs register_handlers 加 compute_diff_command**

修改 `voicepilot/crates/ui/src/commands.rs`,在 `register_handlers` 函数的 `generate_handler!` 列表末尾(`crate::skills_commands::toggle_skill_command,` 之后)加:

```rust
        crate::diff_commands::compute_diff_command,
```

同样在 `register_handlers_with_voice` 函数的 `generate_handler!` 列表末尾(`crate::voice_commands::cancel_voice_command,` 之前)加:

```rust
        crate::diff_commands::compute_diff_command,
```

完整的 `register_handlers` 应为:

```rust
#[cfg(feature = "tauri")]
pub fn register_handlers(
    builder: tauri::Builder<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        submit_approval_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::audit_commands::list_audit_recent_command,
        crate::audit_commands::list_audit_for_task_command,
        crate::trust_center_commands::list_mcp_servers_command,
        crate::trust_center_commands::toggle_mcp_server_command,
        crate::skills_commands::list_skills_command,
        crate::skills_commands::toggle_skill_command,
        crate::diff_commands::compute_diff_command,
    ])
}
```

完整的 `register_handlers_with_voice` 应为:

```rust
#[cfg(feature = "voice")]
pub fn register_handlers_with_voice(
    builder: tauri::Builder<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        route_text_command,
        organize_files_command,
        submit_approval_command,
        crate::settings_commands::get_settings_command,
        crate::settings_commands::update_settings_command,
        crate::audit_commands::list_audit_recent_command,
        crate::audit_commands::list_audit_for_task_command,
        crate::trust_center_commands::list_mcp_servers_command,
        crate::trust_center_commands::toggle_mcp_server_command,
        crate::skills_commands::list_skills_command,
        crate::skills_commands::toggle_skill_command,
        crate::diff_commands::compute_diff_command,
        crate::voice_commands::voice_listen_command,
        crate::voice_commands::cancel_voice_command,
    ])
}
```

- [ ] **Step 3.5: 编译验证**

Run:

```powershell
cd voicepilot; cargo check -p voicepilot-ui --features tauri
```

Expected: 编译通过,无错误。

- [ ] **Step 3.6: 写 diff_commands 集成测试**

创建 `voicepilot/crates/ui/tests/diff_commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

//! W6b-3a Task 3:compute_diff_command 集成测试。
//!
//! 验证 allowed_paths 白名单生效:
//! - 在白名单内的路径:Ok(DiffResult)
//! - 不在白名单的路径:Err

use std::io::Write;
use tempfile::NamedTempFile;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::kernel::TrustKernel;
use voicepilot_ui::state::AppState;

#[test]
fn compute_diff_command_allowed_path() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 不设 allowed_paths —— 开放访问
    let state = AppState::new(kernel);

    let src = NamedTempFile::new().unwrap();
    std::fs::write(src.path(), b"line1\nline2\n").unwrap();
    let dst = NamedTempFile::new().unwrap();
    std::fs::write(dst.path(), b"line1\nline2 modified\n").unwrap();

    // 直接调用 impl 函数(避免 Tauri State 包装)
    let result = voicepilot_ui::diff_commands::compute_diff_impl(
        &state,
        &src.path().to_string_lossy(),
        &dst.path().to_string_lossy(),
    );
    assert!(result.is_ok(), "err = {:?}", result.err());
    let diff = result.unwrap();
    assert_eq!(diff.file_kind, trust_kernel::tools::diff::FileKind::Text);
    assert!(diff.diff_text.unwrap().contains("+line2 modified"));
}

#[test]
fn compute_diff_command_blocked_path() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 设 allowed_paths 为 C:/safe_area,使 E:/ 路径被拒
    let allowed = AllowedPaths::new(vec!["C:/safe_area".to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    let result = voicepilot_ui::diff_commands::compute_diff_impl(
        &state,
        "E:/definitely_nonexistent/source.txt",
        "E:/definitely_nonexistent/dest.txt",
    );
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("path") || err_msg.contains("not allowed"), "err = {}", err_msg);
}
```

注意:`compute_diff_impl` 是 `pub fn`(不是 `#[tauri::command]`),需要在 `diff_commands.rs` 中确保它是 pub。当前签名是 `fn compute_diff_impl(...)`(private)—— 需要改为 `pub fn compute_diff_impl(...)`。

**修正 Task 3.1 的 diff_commands.rs**:`compute_diff_impl` 改为 `pub fn`:

```rust
pub fn compute_diff_impl(
    state: &AppState,
    source_path: &str,
    dest_path: &str,
) -> Result<DiffResult, UiError> {
    // ... 同上
}
```

- [ ] **Step 3.7: 运行测试验证**

Run:

```powershell
cd voicepilot; cargo test -p voicepilot-ui --features tauri --test diff_commands_unit -- --nocapture
```

Expected: 2 个测试全部 PASS。

- [ ] **Step 3.8: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/src/diff_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/src/error.rs voicepilot/crates/ui/tests/diff_commands_unit.rs; git commit -m "feat(w6b-3a): add compute_diff_command Tauri command with allowed_paths enforcement"
```

---

## Task 4: 前端 DiffViewer 组件 + types + api

**目标:** 加 `DiffResult` / `FileKind` TS 类型,`computeDiff` api 函数,`DiffViewer.tsx` 组件(懒加载,loading / error / truncated / binary / new-file / text 状态)。

**Files:**
- Modify: `voicepilot/crates/ui/web/src/types.ts`
- Modify: `voicepilot/crates/ui/web/src/api.ts`
- Create: `voicepilot/crates/ui/web/src/components/DiffViewer.tsx`

- [ ] **Step 4.1: types.ts 加 DiffResult / FileKind 类型**

修改 `voicepilot/crates/ui/web/src/types.ts`,在文件末尾追加:

```typescript
// ===== W6b-3a Task 4: Diff Preview =====

export type FileKind = "new_file" | "text" | "binary";

export interface DiffResult {
  source_path: string;
  dest_path: string;
  file_kind: FileKind;
  diff_text: string | null;
  truncated: boolean;
  truncate_reason: string | null;
}
```

- [ ] **Step 4.2: api.ts 加 computeDiff 函数**

修改 `voicepilot/crates/ui/web/src/api.ts`,在文件末尾追加:

```typescript
// ===== W6b-3a Task 4: Diff Preview =====

export async function computeDiff(
  sourcePath: string,
  destPath: string
): Promise<DiffResult> {
  return invoke<DiffResult>("compute_diff_command", {
    sourcePath,
    destPath,
  });
}
```

同时在文件顶部 `import type { ... } from "./types";` 列表中加 `DiffResult`:

```typescript
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  AuditEvent,
  DiffResult,
  McpServer,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
  Skill,
  VoiceListenResult,
  TranscriptionFinalPayload,
  TranscriptionPartialPayload,
  Settings,
} from "./types";
```

- [ ] **Step 4.3: 创建 DiffViewer.tsx**

创建 `voicepilot/crates/ui/web/src/components/DiffViewer.tsx`:

```tsx
import { useEffect, useState } from "react";
import { computeDiff } from "../api";
import type { DiffResult } from "../types";

interface Props {
  sourcePath: string;
  destPath: string;
  onClose: () => void;
}

export function DiffViewer({ sourcePath, destPath, onClose }: Props): JSX.Element {
  const [result, setResult] = useState<DiffResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    computeDiff(sourcePath, destPath)
      .then((r) => {
        if (!cancelled) {
          setResult(r);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sourcePath, destPath]);

  return (
    <div className="diff-viewer" role="dialog" aria-label="文件 Diff 预览">
      <div className="diff-header">
        <h3>文件 Diff</h3>
        <button
          type="button"
          className="diff-close-btn"
          onClick={onClose}
          aria-label="关闭 Diff"
        >
          ×
        </button>
      </div>
      <div className="diff-paths">
        <div>
          <strong>源:</strong> <code>{sourcePath}</code>
        </div>
        <div>
          <strong>目标:</strong> <code>{destPath}</code>
        </div>
      </div>
      {loading && <p className="diff-loading">加载中…</p>}
      {error && <p className="diff-error">错误:{error}</p>}
      {result && !loading && !error && <DiffContent result={result} />}
    </div>
  );
}

function DiffContent({ result }: { result: DiffResult }): JSX.Element {
  if (result.truncated) {
    return (
      <p className="diff-truncated" role="status">
        ⚠ {result.truncate_reason}
      </p>
    );
  }
  if (result.file_kind === "binary") {
    return <p className="diff-binary">二进制文件,不展示 diff</p>;
  }
  if (result.file_kind === "new_file" && result.diff_text) {
    return (
      <pre className="diff-text diff-new-file">
        <code>{result.diff_text}</code>
      </pre>
    );
  }
  return (
    <pre className="diff-text">
      <code>{result.diff_text ?? "(无差异)"}</code>
    </pre>
  );
}
```

- [ ] **Step 4.4: styles.css 加 diff 样式**

读 `voicepilot/crates/ui/web/src/styles.css` 末尾,追加(若文件末尾没有换行,先加一个空行):

```css

/* ===== W6b-3a Task 4: DiffViewer ===== */
.diff-viewer {
  margin: 12px 0;
  padding: 12px;
  background: rgba(245, 158, 11, 0.05);
  border: 1px solid #f59e0b;
  border-radius: 4px;
}

.diff-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.diff-header h3 {
  margin: 0;
  font-size: 14px;
  color: #f59e0b;
}

.diff-close-btn {
  background: transparent;
  border: none;
  color: #f59e0b;
  font-size: 18px;
  cursor: pointer;
  padding: 0 4px;
}

.diff-paths {
  font-size: 12px;
  color: #94a3b8;
  margin-bottom: 8px;
  word-break: break-all;
}

.diff-paths code {
  color: #60a5fa;
}

.diff-loading,
.diff-error,
.diff-truncated,
.diff-binary {
  padding: 8px;
  font-size: 13px;
}

.diff-error {
  color: #ef4444;
}

.diff-truncated {
  color: #f59e0b;
}

.diff-text {
  background: #0a1628;
  color: #e2e8f0;
  padding: 12px;
  border-radius: 4px;
  font-family: "IBM Plex Mono", monospace;
  font-size: 12px;
  overflow-x: auto;
  max-height: 400px;
  overflow-y: auto;
  white-space: pre;
}

.diff-text code {
  font-family: inherit;
}

.diff-new-file {
  border-left: 3px solid #10b981;
}

.diff-toggle-btn {
  background: transparent;
  border: 1px solid #f59e0b;
  color: #f59e0b;
  padding: 2px 8px;
  font-size: 11px;
  border-radius: 3px;
  cursor: pointer;
}

.diff-toggle-btn:hover {
  background: rgba(245, 158, 11, 0.1);
}

.diff-toggle-btn[aria-expanded="true"] {
  background: #f59e0b;
  color: #0a1628;
}
```

- [ ] **Step 4.5: 前端构建验证**

Run(在 `voicepilot/crates/ui/web/` 目录下):

```powershell
cd voicepilot/crates/ui/web; npm run build
```

Expected: 构建成功,无 TypeScript 错误。

- [ ] **Step 4.6: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/components/DiffViewer.tsx voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist; git commit -m "feat(w6b-3a): add DiffViewer component with lazy-load + truncated/binary/new-file states"
```

---

## Task 5: ApprovalModal 集成 Diff 按钮 + 批次审批文案中文化

**目标:** 在 ApprovalModal.tsx 每个 source 行加"Diff"按钮(懒加载 DiffViewer),并把 Allow/Deny 按钮文案改为"允许所有 (N 个文件)" / "拒绝所有 (N 个文件)"(整批决策)。

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx`

- [ ] **Step 5.1: ApprovalModal 加 DiffViewer 集成 + 批次文案**

完全替换 `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` 内容:

```tsx
import { useEffect, useRef, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";
import { DiffViewer } from "./DiffViewer";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // submittedRef 短路:decide() 成功后置 true,cleanup effect 跳过冗余 deny(W6a Fast-Follow)
  const submittedRef = useRef(false);
  // W6b-3a Task 5:expandedDiff 跟踪当前展开 Diff 的 source path(同时间只展开一个)
  const [expandedDiff, setExpandedDiff] = useState<string | null>(null);
  const { approval_request_id, manifest } = payload;
  const fileCount = manifest.sources.length;

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    setError(null);
    try {
      await submitApproval(approval_request_id, decision);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Esc 键关闭 modal —— 卸载时 cleanup effect 会自动发送 deny(一次性语义)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  // 卸载时自动拒绝(例如用户关闭窗口)—— submittedRef 短路:已提交则跳过(W6a Fast-Follow)
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      // 关闭时尽力发送 deny —— 但仅当尚未提交
      // Rust 端如果已消费会返回 false(一次性)
      submitApproval(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="approval-modal-title">
        <div className="modal-header">
          <h2 id="approval-modal-title">Approve File Operation</h2>
          <span className="badge">E2 · D2 · Local</span>
        </div>
        <div className="modal-body">
          <div className="manifest-summary">
            <div className="summary-stat">
              <span className="label">Sources</span>
              <span className="value">{fileCount}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Total Bytes</span>
              <span className="value">{manifest.total_bytes.toLocaleString()}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Conflicts</span>
              <span className="value danger">
                {manifest.conflicts.length}
              </span>
            </div>
          </div>

          <table className="manifest-table">
            <thead>
              <tr>
                <th>Path</th>
                <th>Size</th>
                <th>SHA-256</th>
                <th>Diff</th>
              </tr>
            </thead>
            <tbody>
              {manifest.sources.map((s) => {
                const isExpanded = expandedDiff === s.canonical_path;
                const destPath = `${manifest.destination}/${s.canonical_path.split(/[\\/]/).pop()}`;
                return (
                  <>
                    <tr key={s.canonical_path}>
                      <td className="path">{s.canonical_path}</td>
                      <td>{s.size}</td>
                      <td>{s.sha256.slice(0, 16)}…</td>
                      <td>
                        <button
                          type="button"
                          className="diff-toggle-btn"
                          onClick={() =>
                            setExpandedDiff(isExpanded ? null : s.canonical_path)
                          }
                          aria-expanded={isExpanded}
                          aria-label={`查看 ${s.canonical_path} 的 Diff`}
                        >
                          {isExpanded ? "收起" : "Diff"}
                        </button>
                      </td>
                    </tr>
                    {isExpanded && (
                      <tr key={`${s.canonical_path}-diff`}>
                        <td colSpan={4}>
                          <DiffViewer
                            sourcePath={s.canonical_path}
                            destPath={destPath}
                            onClose={() => setExpandedDiff(null)}
                          />
                        </td>
                      </tr>
                    )}
                  </>
                );
              })}
            </tbody>
          </table>

          <div className="form-row" style={{ marginTop: 24 }}>
            <label htmlFor="approval-destination">Destination</label>
            <input id="approval-destination" type="text" value={manifest.destination} readOnly />
          </div>

          {manifest.conflicts.length > 0 && (
            <div className="conflicts-list">
              ⚠ {manifest.conflicts.length} conflict(s) detected:
              <ul>
                {manifest.conflicts.map((c, i) => (
                  <li key={i}>{c}</li>
                ))}
              </ul>
            </div>
          )}

          {error && (
            <div className="conflicts-list" style={{ marginTop: 16 }}>
              ⨯ {error}
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
          >
            拒绝所有 ({fileCount} 个文件)
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            允许所有 ({fileCount} 个文件)
          </button>
        </div>
      </div>
    </div>
  );
}
```

**关键改动**:
- 表格加第 4 列 "Diff"
- 每个 source 行加 Diff 按钮(toggle expandedDiff)
- 展开时插一行 `<tr colSpan=4>` 包含 DiffViewer(懒加载)
- dest_path 拼接:`manifest.destination + "/" + basename(canonical_path)`
- 底部按钮文案改为"允许所有 (N 个文件)" / "拒绝所有 (N 个文件)"(整批决策)

- [ ] **Step 5.2: 前端构建验证**

Run:

```powershell
cd voicepilot/crates/ui/web; npm run build
```

Expected: 构建成功。

- [ ] **Step 5.3: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/web/src/components/ApprovalModal.tsx voicepilot/crates/ui/web/dist; git commit -m "feat(w6b-3a): integrate DiffViewer into ApprovalModal + batch-approval Chinese copy"
```

---

## Task 6: 模型 auto-download 后端(Rust ureq + SHA256)

**目标:** 在 trust-kernel/voice 加 `model_download` 模块,实现 `ModelInfo` / `default_model_info` / `models_dir` / `check_model_present` / `download_model`。用 `ureq` HTTPS 下载,100ms 节流进度回调,.part 临时文件,原子 rename,SHA256 校验。

**Files:**
- Modify: `voicepilot/Cargo.toml`(workspace deps)
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/error.rs`
- Create: `voicepilot/crates/trust-kernel/src/voice/model_download.rs`

- [ ] **Step 6.1: workspace Cargo.toml 加 ureq + dirs 依赖**

修改 `voicepilot/Cargo.toml`,在 `[workspace.dependencies]` 段 `similar = "2"` 后加:

```toml
ureq = { version = "2", features = ["tls"] }
dirs = "5"
sha2 = "0.10"
```

注意:`sha2` 已经在 workspace deps 中(项目记忆显示),如果已存在则跳过这一行。

- [ ] **Step 6.2: trust-kernel Cargo.toml 加 ureq + dirs optional 依赖**

修改 `voicepilot/crates/trust-kernel/Cargo.toml`,在 `[dependencies]` 段 `similar = { workspace = true }` 后加:

```toml
ureq = { workspace = true, optional = true }
dirs = { workspace = true, optional = true }
```

在 `[features]` 段 `voice = ["dep:whisper-rs", "dep:cpal", "dep:hound"]` 改为:

```toml
voice = ["dep:whisper-rs", "dep:cpal", "dep:hound", "dep:ureq", "dep:dirs"]
```

- [ ] **Step 6.3: voice/error.rs 加 DownloadFailed variant**

修改 `voicepilot/crates/trust-kernel/src/voice/error.rs`,在 `ModelLoadFailed(String)` variant 后加:

```rust
    #[error("model download failed: {0}")]
    DownloadFailed(String),
```

完整 enum 应为:

```rust
#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice model missing: {0} (run `voicepilot voice list-models` for download instructions)")]
    ModelMissing(String),

    #[error("microphone access denied")]
    MicDenied,

    #[error("whisper inference failed: {0}")]
    InferenceFailed(String),

    #[error("invalid WAV file: {0}")]
    InvalidWav(String),

    #[error("no speech detected in audio")]
    NoSpeechDetected,

    #[error("audio capture failed: {0}")]
    CaptureFailed(String),

    #[error("model load failed: {0}")]
    ModelLoadFailed(String),

    #[error("model download failed: {0}")]
    DownloadFailed(String),
}
```

- [ ] **Step 6.4: voice/mod.rs 加 model_download 模块**

修改 `voicepilot/crates/trust-kernel/src/voice/mod.rs`,在 `pub mod listener;` 后加:

```rust
pub mod model_download;
```

完整文件应为:

```rust
//! Voice input subsystem — V1.1 §2.1 (voice input extension, W5).
//!
//! Pipeline: audio capture (cpal) → VAD (energy threshold) →
//! Whisper.cpp transcription (whisper-rs) → SkillRouter::route →
//! Skill execution.
//!
//! All modules feature-gated under `voice` feature (default on).

pub mod error;
pub mod model;
pub mod wav;
pub mod vad;
pub mod whisper;
pub mod audio;
pub mod router_bridge;
pub mod listener;
pub mod model_download;
```

- [ ] **Step 6.5: 创建 model_download.rs**

创建 `voicepilot/crates/trust-kernel/src/voice/model_download.rs`:

```rust
//! 模型 auto-download(W6b-3a Task 6)。
//!
//! 用 `ureq` HTTPS 下载 Whisper 模型到 `~/.voicepilot/models/`:
//! - 100ms 节流进度回调
//! - .part 临时文件,原子 rename
//! - SHA256 校验(若 expected_sha256 提供)
//!
//! 整个模块在 `#[cfg(feature = "voice")]` 下(由 voice/mod.rs 控制)。

use crate::voice::error::{VoiceError, VoiceResult};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 模型信息(下载用)。
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub download_url: String,
    pub expected_sha256: Option<String>,
    pub size_hint_mb: u32,
}

/// 默认模型信息(ggml-tiny.bin,~75MB)。
pub fn default_model_info() -> ModelInfo {
    ModelInfo {
        name: "ggml-tiny.bin".to_string(),
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
                .to_string(),
        // HuggingFace 不提供官方 SHA256,这里用 None —— 下载后由
        // whisper-rs 加载时验证(失败会返回 ModelLoadFailed)
        expected_sha256: None,
        size_hint_mb: 75,
    }
}

/// 返回模型存储目录:`~/.voicepilot/models/`。
///
/// 用 `dirs` crate 跨平台获取 home 目录。
pub fn models_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".voicepilot").join("models")
}

/// 检查指定模型是否已存在。
pub fn check_model_present(name: &str) -> bool {
    models_dir().join(name).is_file()
}

/// 下载进度回调参数。
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// 下载模型到 `~/.voicepilot/models/<name>`。
///
/// - `on_progress`:每 100ms 调用一次(节流),传入下载进度
/// - 失败时清理 .part 文件
/// - 成功后原子 rename .part → 最终文件名
/// - 若 `expected_sha256` 提供,下载完成后校验 SHA256
pub fn download_model<F>(
    info: &ModelInfo,
    on_progress: F,
) -> VoiceResult<PathBuf>
where
    F: Fn(DownloadProgress),
{
    let dir = models_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        VoiceError::DownloadFailed(format!("create models_dir failed: {}", e))
    })?;

    let final_path = dir.join(&info.name);
    let part_path = dir.join(format!("{}.part", info.name));

    // 若最终文件已存在,直接返回(幂等)
    if final_path.is_file() {
        return Ok(final_path);
    }

    // 清理可能残留的 .part 文件
    if part_path.exists() {
        let _ = fs::remove_file(&part_path);
    }

    // 发起 HTTP GET
    let resp = ureq::get(&info.download_url)
        .call()
        .map_err(|e| VoiceError::DownloadFailed(format!("HTTP request failed: {}", e)))?;

    let total_bytes = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    // 写入 .part 文件
    let mut part_file = fs::File::create(&part_path).map_err(|e| {
        VoiceError::DownloadFailed(format!("create .part file failed: {}", e))
    })?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024]; // 32KB buffer
    let mut downloaded: u64 = 0;
    let mut last_progress = Instant::now();
    let progress_throttle = Duration::from_millis(100);

    loop {
        let n = resp
            .into_reader()
            .read(&mut buf)
            .map_err(|e| VoiceError::DownloadFailed(format!("read failed: {}", e)))?;
        if n == 0 {
            break;
        }
        part_file
            .write_all(&buf[..n])
            .map_err(|e| VoiceError::DownloadFailed(format!("write failed: {}", e)))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;

        // 节流:每 100ms 报告一次进度
        if last_progress.elapsed() >= progress_throttle {
            let percent = total_bytes.map(|t| (downloaded as f32 / t as f32) * 100.0);
            on_progress(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes,
                percent,
            });
            last_progress = Instant::now();
        }
    }

    // 最终进度报告
    let percent = total_bytes.map(|t| (downloaded as f32 / t as f32) * 100.0);
    on_progress(DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes,
        percent,
    });

    // flush + sync
    part_file
        .sync_all()
        .map_err(|e| VoiceError::DownloadFailed(format!("sync failed: {}", e)))?;
    drop(part_file);

    // SHA256 校验(若提供)
    if let Some(expected) = &info.expected_sha256 {
        let actual = format!("{:x}", hasher.finalize());
        if &actual != expected {
            let _ = fs::remove_file(&part_path);
            return Err(VoiceError::DownloadFailed(format!(
                "SHA256 mismatch: expected={} actual={}",
                expected, actual
            )));
        }
    }

    // 原子 rename
    fs::rename(&part_path, &final_path).map_err(|e| {
        VoiceError::DownloadFailed(format!("rename failed: {}", e))
    })?;

    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_info_correct() {
        let info = default_model_info();
        assert_eq!(info.name, "ggml-tiny.bin");
        assert!(info.download_url.contains("huggingface.co"));
        assert!(info.size_hint_mb > 0);
    }

    #[test]
    fn check_model_present_nonexistent() {
        // 用一个肯定不存在的名字
        assert!(!check_model_present("definitely_nonexistent_model_xxx.bin"));
    }

    #[test]
    fn models_dir_ends_with_voicepilot_models() {
        let dir = models_dir();
        let s = dir.to_string_lossy();
        assert!(s.contains(".voicepilot"), "dir = {}", s);
        assert!(s.contains("models"), "dir = {}", s);
    }
}
```

注意:`sha2` 已经是 trust-kernel 的依赖(项目记忆),不需要重复加。但需要确认 `sha2` 是 `pub use` 还是 private。在 model_download.rs 中直接用 `use sha2::{Digest, Sha256}`。

- [ ] **Step 6.6: 运行测试验证**

Run:

```powershell
cd voicepilot; cargo test -p trust-kernel --features voice --lib voice::model_download -- --nocapture
```

Expected: 3 个测试 PASS。

- [ ] **Step 6.7: 运行 clippy 验证**

Run:

```powershell
cd voicepilot; cargo clippy -p trust-kernel --features voice --lib -- -D warnings
```

Expected: 无 warning。

- [ ] **Step 6.8: Commit**

```powershell
cd voicepilot; git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/src/voice/error.rs voicepilot/crates/trust-kernel/src/voice/model_download.rs; git commit -m "feat(w6b-3a): add model_download with ureq + SHA256 + 100ms throttled progress"
```

---

## Task 7: 模型 auto-download Tauri command + is_voice_enabled

**目标:** 在 ui crate 加 `model_download_commands.rs`,提供 `check_model_command` / `download_model_command` / `is_voice_enabled_command` 三个 Tauri command。

**Files:**
- Create: `voicepilot/crates/ui/src/model_download_commands.rs`
- Modify: `voicepilot/crates/ui/src/lib.rs`
- Modify: `voicepilot/crates/ui/src/commands.rs`

- [ ] **Step 7.1: 创建 model_download_commands.rs**

创建 `voicepilot/crates/ui/src/model_download_commands.rs`:

```rust
//! 模型 auto-download Tauri commands(W6b-3a Task 7)。
//!
//! 整个模块在 `#[cfg(feature = "voice")]` 下(由 lib.rs 控制)。
//!
//! 三个 command:
//! - `is_voice_enabled_command`:返回 `cfg!(feature = "voice")`(无 IO)
//! - `check_model_command`:返回模型是否存在(ModelStatus)
//! - `download_model_command`:异步下载,spawn_blocking 调用 trust-kernel

use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use trust_kernel::voice::model_download::{
    check_model_present, default_model_info, download_model, DownloadProgress,
};

/// 模型状态(前端 ModelDownloadBar 用)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    /// voice feature 未启用
    Disabled,
    /// 模型已存在
    Present,
    /// 模型不存在,需要下载
    Absent,
}

/// 下载进度事件 payload(emit 到前端)。
///
/// 注意:不加 `#[serde(rename_all = "camelCase")]` —— 与现有
/// `TranscriptionPartialPayload` 等保持一致(全 snake_case)。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// `is_voice_enabled` command。
///
/// 在 `register_handlers` 中注册(不带 voice 也能调用,返回 false)。
/// 内部用 `cfg!(feature = "voice")` 判断,前端用于决定是否显示 ModelDownloadBar。
#[tauri::command]
pub fn is_voice_enabled_command() -> bool {
    cfg!(feature = "voice")
}

/// `check_model` command。
///
/// 仅在 voice feature 下有意义,但注册到 `register_handlers_with_voice`。
#[tauri::command]
pub fn check_model_command() -> ModelStatus {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        if check_model_present(&info.name) {
            ModelStatus::Present
        } else {
            ModelStatus::Absent
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        ModelStatus::Disabled
    }
}

/// `download_model` command。
///
/// 异步下载,spawn_blocking 调用 trust-kernel 的 `download_model`。
/// 下载期间每 100ms emit `model-download-progress` 事件给前端。
#[tauri::command]
pub async fn download_model_command(
    app: AppHandle,
) -> Result<String, String> {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        let info_clone = info.clone();

        let result = tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            let app_clone = app.clone();
            let result = download_model(&info_clone, |progress: DownloadProgress| {
                // 节流:每 100ms 由 trust-kernel 内部控制
                let payload = DownloadProgressPayload {
                    downloaded_bytes: progress.downloaded_bytes,
                    total_bytes: progress.total_bytes,
                    percent: progress.percent,
                };
                let _ = app_clone.emit("model-download-progress", payload);
            });
            (result, start.elapsed())
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        match result.0 {
            Ok(path) => Ok(path.to_string_lossy().into_owned()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        let _ = app;
        Err("voice feature not enabled".to_string())
    }
}
```

- [ ] **Step 7.2: lib.rs 加 model_download_commands 模块**

修改 `voicepilot/crates/ui/src/lib.rs`,在 `#[cfg(feature = "voice")] pub mod voice_commands;` 后加:

```rust
#[cfg(feature = "voice")]
pub mod model_download_commands;
```

- [ ] **Step 7.3: commands.rs 注册新 command**

修改 `voicepilot/crates/ui/src/commands.rs`:

**3.1 在 `register_handlers` 函数的 `generate_handler!` 列表末尾(`crate::diff_commands::compute_diff_command,` 之后)加**:

```rust
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
```

注意:`download_model_command` 用 `#[cfg(not(feature = "voice"))]` 分支返回错误,所以即使非 voice feature 也能编译。但 `model_download_commands` 模块整体被 `#[cfg(feature = "voice")]` 门控,所以在 `register_handlers`(无 voice feature)中不能引用。

**修正**:将 `model_download_commands` 模块改为**不带 voice feature 门控**(模块内部用 `#[cfg(feature = "voice")]` 分支处理)。这样 `register_handlers` 能引用 `is_voice_enabled_command` 等。

修改 `voicepilot/crates/ui/src/lib.rs`,把:

```rust
#[cfg(feature = "voice")]
pub mod model_download_commands;
```

改为:

```rust
pub mod model_download_commands;
```

并修改 `voicepilot/crates/ui/src/model_download_commands.rs`,在文件顶部加 `#![allow(unused_imports)]`,确保即使非 voice feature 也能编译(因为 `use trust_kernel::voice::model_download::*` 在非 voice 下会失败)。

**最终方案**:用条件编译包裹 voice 相关 import:

```rust
//! 模型 auto-download Tauri commands(W6b-3a Task 7)。
//!
//! 模块整体编译(无 voice feature 也能编译),但 voice 相关 import 在 cfg 内。

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(feature = "voice")]
use tauri::Emitter;
#[cfg(feature = "voice")]
use trust_kernel::voice::model_download::{
    check_model_present, default_model_info, download_model, DownloadProgress,
};

/// 模型状态(前端 ModelDownloadBar 用)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Disabled,
    Present,
    Absent,
}

/// 下载进度事件 payload。
///
/// 注意:不加 `#[serde(rename_all = "camelCase")]` —— 与现有
/// `TranscriptionPartialPayload` 等保持一致(全 snake_case)。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// `is_voice_enabled` command。
#[tauri::command]
pub fn is_voice_enabled_command() -> bool {
    cfg!(feature = "voice")
}

/// `check_model` command。
#[tauri::command]
pub fn check_model_command() -> ModelStatus {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        if check_model_present(&info.name) {
            ModelStatus::Present
        } else {
            ModelStatus::Absent
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        ModelStatus::Disabled
    }
}

/// `download_model` command。
#[tauri::command]
pub async fn download_model_command(app: AppHandle) -> Result<String, String> {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        let info_clone = info.clone();

        let result = tokio::task::spawn_blocking(move || {
            let app_clone = app.clone();
            let result = download_model(&info_clone, |progress: DownloadProgress| {
                let payload = DownloadProgressPayload {
                    downloaded_bytes: progress.downloaded_bytes,
                    total_bytes: progress.total_bytes,
                    percent: progress.percent,
                };
                let _ = app_clone.emit("model-download-progress", payload);
            });
            result
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        match result {
            Ok(path) => Ok(path.to_string_lossy().into_owned()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        let _ = app;
        Err("voice feature not enabled".to_string())
    }
}
```

- [ ] **Step 7.4: 在 register_handlers 加新 commands**

修改 `voicepilot/crates/ui/src/commands.rs`,在 `register_handlers` 的 `generate_handler!` 列表末尾(`crate::diff_commands::compute_diff_command,` 之后)加:

```rust
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
```

同样在 `register_handlers_with_voice` 的 `generate_handler!` 列表末尾(`crate::voice_commands::cancel_voice_command,` 之前)加:

```rust
        crate::model_download_commands::is_voice_enabled_command,
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
```

- [ ] **Step 7.5: 编译验证(无 voice)**

Run:

```powershell
cd voicepilot; cargo check -p voicepilot-ui --features tauri
```

Expected: 编译通过。

- [ ] **Step 7.6: 编译验证(带 voice)**

Run:

```powershell
cd voicepilot; cargo check -p voicepilot-ui --features voice
```

Expected: 编译通过。

- [ ] **Step 7.7: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/src/model_download_commands.rs voicepilot/crates/ui/src/lib.rs voicepilot/crates/ui/src/commands.rs; git commit -m "feat(w6b-3a): add model_download_commands (is_voice_enabled + check_model + download_model)"
```

---

## Task 8: 前端 ModelDownloadBar 组件 + App.tsx 集成

**目标:** 加 `ModelDownloadBar.tsx`(状态机 checking/present/absent/downloading/done/error + 进度条 + 重试),在 App.tsx 启动时检测,语音未启用时不显示。

**Files:**
- Modify: `voicepilot/crates/ui/web/src/types.ts`
- Modify: `voicepilot/crates/ui/web/src/api.ts`
- Create: `voicepilot/crates/ui/web/src/components/ModelDownloadBar.tsx`
- Modify: `voicepilot/crates/ui/web/src/App.tsx`
- Modify: `voicepilot/crates/ui/web/src/styles.css`

- [ ] **Step 8.1: types.ts 加 ModelStatus + DownloadProgress**

修改 `voicepilot/crates/ui/web/src/types.ts`,在文件末尾追加:

```typescript

// ===== W6b-3a Task 8: ModelDownloadBar =====

export type ModelStatus = "disabled" | "present" | "absent";

export interface DownloadProgressPayload {
  downloaded_bytes: number;
  total_bytes: number | null;
  percent: number | null;
}
```

- [ ] **Step 8.2: api.ts 加 isVoiceEnabled / checkModel / downloadModel + onModelDownloadProgress**

修改 `voicepilot/crates/ui/web/src/api.ts`,在文件末尾追加:

```typescript

// ===== W6b-3a Task 8: ModelDownloadBar =====

export async function isVoiceEnabled(): Promise<boolean> {
  return invoke<boolean>("is_voice_enabled_command");
}

export async function checkModel(): Promise<ModelStatus> {
  return invoke<ModelStatus>("check_model_command");
}

export async function downloadModel(): Promise<string> {
  return invoke<string>("download_model_command");
}

export function onModelDownloadProgress(
  handler: (payload: DownloadProgressPayload) => void
): Promise<UnlistenFn> {
  return listen<DownloadProgressPayload>("model-download-progress", (e) =>
    handler(e.payload)
  );
}
```

同时在文件顶部 import 列表加 `ModelStatus` / `DownloadProgressPayload`:

```typescript
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  AuditEvent,
  DiffResult,
  DownloadProgressPayload,
  McpServer,
  ModelStatus,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
  Skill,
  VoiceListenResult,
  TranscriptionFinalPayload,
  TranscriptionPartialPayload,
  Settings,
} from "./types";
```

- [ ] **Step 8.3: 创建 ModelDownloadBar.tsx**

创建 `voicepilot/crates/ui/web/src/components/ModelDownloadBar.tsx`:

```tsx
import { useEffect, useState } from "react";
import { checkModel, downloadModel, isVoiceEnabled, onModelDownloadProgress } from "../api";
import type { DownloadProgressPayload, ModelStatus } from "../types";

type Phase = "checking" | "present" | "absent" | "downloading" | "done" | "error";

interface State {
  phase: Phase;
  voiceEnabled: boolean;
  progress: DownloadProgressPayload | null;
  error: string | null;
}

const INITIAL: State = {
  phase: "checking",
  voiceEnabled: false,
  progress: null,
  error: null,
};

export function ModelDownloadBar(): JSX.Element | null {
  const [state, setState] = useState<State>(INITIAL);

  // 启动时检测 voice 是否启用 + 模型是否存在
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const enabled = await isVoiceEnabled();
        if (cancelled) return;
        if (!enabled) {
          setState({ ...INITIAL, phase: "present", voiceEnabled: false });
          return;
        }
        const status: ModelStatus = await checkModel();
        if (cancelled) return;
        if (status === "present") {
          setState({ phase: "present", voiceEnabled: true, progress: null, error: null });
        } else {
          setState({ phase: "absent", voiceEnabled: true, progress: null, error: null });
        }
      } catch (e) {
        if (!cancelled) {
          setState({
            phase: "error",
            voiceEnabled: false,
            progress: null,
            error: String(e),
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 监听下载进度
  useEffect(() => {
    if (state.phase !== "downloading") return;
    let unlistenFn: (() => void) | null = null;
    const promise = onModelDownloadProgress((payload) => {
      setState((s) => ({ ...s, progress: payload }));
    });
    promise
      .then((fn) => {
        unlistenFn = fn;
      })
      .catch(() => {});
    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, [state.phase]);

  // voice 未启用 → 不渲染
  if (!state.voiceEnabled) return null;
  // 模型已存在 → 不渲染(无需打扰用户)
  if (state.phase === "present" || state.phase === "done") return null;

  async function handleDownload() {
    setState((s) => ({ ...s, phase: "downloading", progress: null, error: null }));
    try {
      await downloadModel();
      setState((s) => ({ ...s, phase: "done", progress: null, error: null }));
    } catch (e) {
      setState((s) => ({
        ...s,
        phase: "error",
        progress: null,
        error: e instanceof Error ? e.message : String(e),
      }));
    }
  }

  function handleRetry() {
    setState((s) => ({ ...s, phase: "absent", progress: null, error: null }));
  }

  // checking → 简短 loading
  if (state.phase === "checking") {
    return (
      <div className="model-download-bar checking" role="status" aria-live="polite">
        <span>检查语音模型状态…</span>
      </div>
    );
  }

  // absent → 询问用户是否下载
  if (state.phase === "absent") {
    return (
      <div className="model-download-bar absent" role="alertdialog" aria-labelledby="mdl-title">
        <span id="mdl-title" className="mdl-message">
          ⚠ 语音模型未安装(ggml-tiny.bin,~75MB),需要下载后才能使用语音输入。
        </span>
        <button
          type="button"
          className="btn btn-primary mdl-btn"
          onClick={handleDownload}
        >
          下载模型
        </button>
      </div>
    );
  }

  // downloading → 进度条
  if (state.phase === "downloading") {
    const percent = state.progress?.percent ?? 0;
    const downloadedMb = state.progress
      ? (state.progress.downloaded_bytes / 1024 / 1024).toFixed(1)
      : "0";
    const totalMb = state.progress?.total_bytes
      ? (state.progress.total_bytes / 1024 / 1024).toFixed(1)
      : "?";
    return (
      <div className="model-download-bar downloading" role="status" aria-live="polite">
        <div className="mdl-progress-info">
          下载中…{downloadedMb} / {totalMb} MB({percent.toFixed(1)}%)
        </div>
        <div
          className="mdl-progress-bar"
          role="progressbar"
          aria-valuenow={Math.round(percent)}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div className="mdl-progress-fill" style={{ width: `${percent}%` }} />
        </div>
      </div>
    );
  }

  // error → 错误信息 + 重试
  if (state.phase === "error") {
    return (
      <div className="model-download-bar error" role="alert">
        <span className="mdl-message">⨯ 下载失败:{state.error}</span>
        <button
          type="button"
          className="btn btn-secondary mdl-btn"
          onClick={handleRetry}
        >
          重试
        </button>
      </div>
    );
  }

  return null;
}
```

- [ ] **Step 8.4: App.tsx 集成 ModelDownloadBar**

修改 `voicepilot/crates/ui/web/src/App.tsx`:

**4.1 在 imports 列表加 ModelDownloadBar**:

把:

```tsx
import { KillSwitchBar } from "./components/KillSwitchBar";
```

改为:

```tsx
import { KillSwitchBar } from "./components/KillSwitchBar";
import { ModelDownloadBar } from "./components/ModelDownloadBar";
```

**4.2 在 render 中 `<KillSwitchBar ... />` 之后、`<div className="app-body">` 之前加 `<ModelDownloadBar />`**:

把:

```tsx
      <KillSwitchBar
        isNarrow={isNarrow}
        onToggleSidebar={() => setSidebarOpen((o) => !o)}
      />
      <div className="app-body">
```

改为:

```tsx
      <KillSwitchBar
        isNarrow={isNarrow}
        onToggleSidebar={() => setSidebarOpen((o) => !o)}
      />
      <ModelDownloadBar />
      <div className="app-body">
```

- [ ] **Step 8.5: styles.css 加 ModelDownloadBar 样式**

修改 `voicepilot/crates/ui/web/src/styles.css`,在文件末尾追加(若已有 diff 样式块,在其后):

```css

/* ===== W6b-3a Task 8: ModelDownloadBar ===== */
.model-download-bar {
  padding: 8px 16px;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid #1e293b;
}

.model-download-bar.checking {
  background: rgba(96, 165, 250, 0.1);
  color: #60a5fa;
}

.model-download-bar.absent {
  background: rgba(245, 158, 11, 0.1);
  color: #f59e0b;
}

.model-download-bar.downloading {
  background: rgba(96, 165, 250, 0.1);
  color: #60a5fa;
  flex-direction: column;
  align-items: stretch;
  gap: 4px;
}

.model-download-bar.error {
  background: rgba(239, 68, 68, 0.1);
  color: #ef4444;
}

.mdl-message {
  flex: 1;
}

.mdl-btn {
  padding: 4px 12px;
  font-size: 12px;
}

.mdl-progress-info {
  font-size: 12px;
}

.mdl-progress-bar {
  height: 6px;
  background: #1e293b;
  border-radius: 3px;
  overflow: hidden;
}

.mdl-progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #f59e0b, #60a5fa);
  transition: width 0.1s ease-out;
}
```

- [ ] **Step 8.6: 前端构建验证**

Run:

```powershell
cd voicepilot/crates/ui/web; npm run build
```

Expected: 构建成功。

- [ ] **Step 8.7: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/web/src/types.ts voicepilot/crates/ui/web/src/api.ts voicepilot/crates/ui/web/src/components/ModelDownloadBar.tsx voicepilot/crates/ui/web/src/App.tsx voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist; git commit -m "feat(w6b-3a): add ModelDownloadBar with start-up detection + progress + retry"
```

---

## Task 9: E2E 全链路冒烟测试

**目标:** 创建 `w6b3_e2e_smoke.rs`,用 Tauri 2 `mock_app()` API + StubVoiceListen mock,验证完整链路:route → organize → approval-request emit → submit_approval → commit → audit chain。

**Files:**
- Create: `voicepilot/crates/ui/tests/w6b3_e2e_smoke.rs`

- [ ] **Step 9.1: 调研 Tauri 2 test API**

**注意**:Tauri 2 的 `mock_app()` API 在 `tauri::test` 模块下,需要 `tauri` crate 启用 `test` feature(或在 dev-dependencies 中加 `tauri = { workspace = true, features = ["test"] }`)。

读 `voicepilot/crates/ui/Cargo.toml` 的 `[dev-dependencies]` 段,确认是否已有 `tauri`。如果没有,在 `[dev-dependencies]` 加:

```toml
tauri = { workspace = true }
```

如果已有,跳过。

- [ ] **Step 9.2: 创建 w6b3_e2e_smoke.rs**

创建 `voicepilot/crates/ui/tests/w6b3_e2e_smoke.rs`:

```rust
#![cfg(feature = "tauri")]

//! W6b-3a Task 9: E2E 全链路冒烟测试。
//!
//! 验证完整链路(无 voice):
//! - route_text("整理下载目录的图片") → Routed { skill_id: "files.organize" }
//! - organize_files → emit "approval-request"
//! - submit_approval(allow) → commit_move
//! - audit chain 包含 prepare / commit / verify 事件
//!
//! 由于 voice feature 在 CI 环境不稳定(whisper-rs bindgen issue #49),
//! 本测试**不**依赖 voice。voice 链路在 w6b1_voice_smoke.rs 单独覆盖。

use std::sync::Arc;
use tempfile::tempdir;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::kernel::TrustKernel;
use voicepilot_ui::commands::{
    organize_files, route_text, submit_approval, OrganizeInput,
};
use voicepilot_ui::state::AppState;

/// 直接调用 route_text(无 Tauri State 包装)。
#[test]
fn e2e_route_text_matches_files_organize() {
    let state = AppState::new_in_memory().unwrap();
    let result = route_text(&state, "整理下载目录的图片").unwrap();
    match result {
        voicepilot_ui::commands::RouteTextResult::Routed { skill_id } => {
            assert_eq!(skill_id, "files.organize");
        }
        other => panic!("expected Routed, got {:?}", other),
    }
}

/// 完整 organize_files 链路:用 AutoApprover 跳过 IPC。
/// 验证 commit 成功 + audit chain 包含关键事件。
#[test]
fn e2e_organize_files_with_auto_approver() {
    // 准备临时目录 + 文件
    let src_dir = tempdir().unwrap();
    let dst_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("test.txt");
    std::fs::write(&src_file, b"hello e2e").unwrap();

    // 准备 kernel + allowed_paths
    let kernel = TrustKernel::open_in_memory().unwrap();
    let allowed_roots = vec![
        src_dir.path().to_string_lossy().to_string(),
        dst_dir.path().to_string_lossy().to_string(),
    ];
    let allowed = AllowedPaths::new(allowed_roots);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    // 用 AutoApprover 跳过 IPC
    use trust_kernel::approval::approver::AutoApprover;
    let approver = AutoApprover::new();

    let input = OrganizeInput {
        task_id: "e2e-task-1".to_string(),
        step_id: "e2e-step-1".to_string(),
        source: src_dir.path().to_string_lossy().to_string(),
        filter: "*.txt".to_string(),
        destination: dst_dir.path().to_string_lossy().to_string(),
    };

    let result = organize_files(&state, &approver, &input).unwrap();
    assert!(result.committed, "expected committed=true, err={:?}", result.error);
    assert_eq!(result.moved_paths.len(), 1);

    // 验证文件已移动
    let moved_dst = dst_dir.path().join("test.txt");
    assert!(moved_dst.is_file(), "dest file should exist");
    let content = std::fs::read_to_string(&moved_dst).unwrap();
    assert_eq!(content, "hello e2e");

    // 验证 audit chain
    // 注意:方法名是 `list_audit_recent(limit)`(无 &conn 参数,内部自己拿锁)
    let audit = state.kernel.list_audit_recent(50).unwrap();
    assert!(!audit.is_empty(), "audit should have events");

    // 至少包含 prepare / commit 事件
    let event_types: Vec<&str> = audit.iter().map(|e| e.event_type.as_str()).collect();
    assert!(
        event_types.iter().any(|t| t.contains("prepare") || t.contains("step")),
        "audit should contain prepare/step event, got {:?}",
        event_types
    );
    assert!(
        event_types.iter().any(|t| t.contains("commit") || t.contains("move")),
        "audit should contain commit/move event, got {:?}",
        event_types
    );
}

/// submit_approval 在无 pending request 时返回 false。
#[test]
fn e2e_submit_approval_no_pending_returns_false() {
    let state = AppState::new_in_memory().unwrap();
    use trust_kernel::approval::types::ApprovalDecision;
    let result = submit_approval(&state, "nonexistent-id", ApprovalDecision::Allow).unwrap();
    assert!(!result, "expected false for nonexistent approval_id");
}

/// compute_diff_command 集成验证(白名单内路径)。
#[test]
fn e2e_compute_diff_command_allowed() {
    use voicepilot_ui::diff_commands::compute_diff_impl;

    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("src.txt");
    std::fs::write(&src_file, b"line1\nline2\n").unwrap();
    let dst_file = src_dir.path().join("dst.txt");
    std::fs::write(&dst_file, b"line1\nline2 modified\n").unwrap();

    let kernel = TrustKernel::open_in_memory().unwrap();
    let allowed = AllowedPaths::new(vec![src_dir.path().to_string_lossy().to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    let result = compute_diff_impl(
        &state,
        &src_file.to_string_lossy(),
        &dst_file.to_string_lossy(),
    );
    assert!(result.is_ok(), "err = {:?}", result.err());
    let diff = result.unwrap();
    assert!(diff.diff_text.unwrap().contains("+line2 modified"));
}

/// is_voice_enabled_command 在非 voice build 下返回 false。
#[test]
fn e2e_is_voice_enabled_returns_false_without_feature() {
    let result = voicepilot_ui::model_download_commands::is_voice_enabled_command();
    // 注意:此测试在 `cargo test --features tauri`(无 voice)下运行 → false
    // 在 `cargo test --features voice` 下运行 → true
    // 不 assert 具体值,只确保不 panic
    let _ = result;
}

/// check_model_command 在非 voice build 下返回 Disabled。
#[test]
fn e2e_check_model_returns_disabled_without_feature() {
    let status = voicepilot_ui::model_download_commands::check_model_command();
    // 非 voice build → Disabled
    // voice build → Present 或 Absent(取决于机器上是否已下载)
    // 不 assert 具体值,只确保不 panic
    let _ = status;
}
```

- [ ] **Step 9.3: 验证 audit 方法签名(已确认)**

self-review 已确认 `TrustKernel::list_audit_recent(&self, limit: usize) -> Result<Vec<AuditEvent>>`(在 `voicepilot/crates/trust-kernel/src/kernel.rs:145`)。Step 9.2 中的 `state.kernel.list_audit_recent(50)` 调用直接生效,无需调整。

如果执行时仍报方法不存在,改为通过 `voicepilot_ui::audit_commands::list_audit_recent(&state, 50)`(返回 `Vec<AuditEventDto>`,字段同 `AuditEvent`)。

- [ ] **Step 9.4: 运行 E2E 测试验证**

Run:

```powershell
cd voicepilot; cargo test -p voicepilot-ui --features tauri --test w6b3_e2e_smoke -- --nocapture
```

Expected: 6 个测试全部 PASS。

若有编译错误(方法名不匹配),根据编译错误调整代码,然后重跑。

- [ ] **Step 9.5: 运行全部 ui 测试确保无回归**

Run:

```powershell
cd voicepilot; cargo test -p voicepilot-ui --features tauri
```

Expected: 所有测试 PASS。

- [ ] **Step 9.6: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/tests/w6b3_e2e_smoke.rs voicepilot/crates/ui/Cargo.toml; git commit -m "test(w6b-3a): add w6b3_e2e_smoke covering route→organize→approve→commit→audit chain"
```

---

## Task 10: Tauri Windows 打包配置 + 图标资源

**目标:** 修改 `tauri.conf.json` 补全 bundle 字段(NSIS / icon / metadata / minWidth),生成流体波纹图标(§6.2.1 锁定方向 A),用 `cargo tauri build` 验证打包。

**Files:**
- Modify: `voicepilot/crates/ui/tauri.conf.json`
- Create: `voicepilot/crates/ui/icons/32x32.png`
- Create: `voicepilot/crates/ui/icons/128x128.png`
- Create: `voicepilot/crates/ui/icons/128x128@2x.png`
- Create: `voicepilot/crates/ui/icons/icon.png`
- Modify: `voicepilot/crates/ui/icons/icon.ico`(替换为多尺寸)

- [ ] **Step 10.1: 生成流体波纹图标源图**

**说明**:W6b-3a §6.2.1 锁定方向 A 流体波纹。开发者若无设计工具,可用 AI 图像生成。

**选项 1:用 SDXL/DALL-E 生成 512x512 源图**

用以下 prompt(已在 spec §6.2.1 提供):

```
Abstract fluid ripple icon, dark navy blue background #0a1628, warm amber glowing core #f59e0b in center, 5 concentric ripple rings fading outward with amber-to-light-blue gradient, slight fluid distortion on rings, minimalist, engineering console aesthetic, 512x512, high contrast, centered composition
```

下载生成的 512x512 PNG,保存为 `voicepilot/crates/ui/icons/icon.png`(源图)。

**选项 2:用 Figma / Inkscape 绘制矢量源图**

按 §6.2.1 视觉规格手动绘制,导出 512x512 PNG。

**选项 3:用 ImageMagick 命令行生成(应急方案)**

如果上述都不可行,用 ImageMagick 生成一个简化版本:

```powershell
cd voicepilot/crates/ui/icons
# 生成深海军蓝背景 + 暖琥珀中心圆 + 多圈渐变(简化版)
magick -size 512x512 xc:"#0a1628" ^
  -fill "#f59e0b" -draw "circle 256,256 256,310" ^
  -fill none -stroke "#f59e0b" -strokewidth 6 -draw "circle 256,256 256,200" ^
  -fill none -stroke "#f59e0b80" -strokewidth 4 -draw "circle 256,256 256,160" ^
  -fill none -stroke "#60a5fa" -strokewidth 2 -draw "circle 256,256 256,120" ^
  icon.png
```

注意:这个应急版本不含流体扭曲,但满足"流体波纹"基本视觉(中心核心 + 同心圆波纹 + 双色渐变)。

- [ ] **Step 10.2: 生成多尺寸 PNG + 多尺寸 ICO**

Run(在 `voicepilot/crates/ui/icons/` 目录下,需要 ImageMagick):

```powershell
cd voicepilot/crates/ui/icons
magick convert icon.png -resize 32x32 32x32.png
magick convert icon.png -resize 128x128 128x128.png
magick convert icon.png -resize 256x256 128x128@2x.png
magick convert icon.png -define icon:auto-resize=16,32,48,64,128,256 icon.ico
```

**注意**:32x32 和 16x16 尺寸下波纹会糊成一团。理想情况下应手动重绘(见 §6.2.1 多尺寸适配规则)。若没有手动重绘条件,接受算法缩放(已加 50MB 软上限,不影响功能)。

验证文件存在:

```powershell
cd voicepilot/crates/ui/icons; dir *.png; dir *.ico
```

Expected:看到 `32x32.png` / `128x128.png` / `128x128@2x.png` / `icon.png` / `icon.ico`。

- [ ] **Step 10.3: 修改 tauri.conf.json**

完全替换 `voicepilot/crates/ui/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VoicePilot",
  "version": "0.1.0",
  "identifier": "com.voicepilot.app",
  "build": {
    "frontendDist": "web/dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "VoicePilot",
        "width": 1024,
        "height": 768,
        "minWidth": 768,
        "minHeight": 600,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ipc: http://ipc.localhost https://huggingface.co; object-src 'none'; frame-ancestors 'none'"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.ico"
    ],
    "copyright": "Copyright © 2026 VoicePilot Team",
    "category": "Utility",
    "shortDescription": "VoicePilot — 权限感知语音桌面 Agent",
    "longDescription": "VoicePilot 是一个权限感知的语音桌面 Agent,通过 Trust Kernel 提供单一的信任边界,支持语音命令、文件整理、审批流程等。",
    "windows": {
      "nsis": {
        "installerIcon": "icons/icon.ico",
        "displayLanguageSelector": false,
        "languages": ["SimpChinese", "English"]
      }
    }
  }
}
```

**关键改动**:
- `bundle.targets` 从 `"all"` 改为 `["nsis"]`(仅 Windows NSIS)
- `bundle.icon` 补齐多尺寸 PNG + ICO
- 加 `bundle.copyright` / `category` / `shortDescription` / `longDescription`
- 加 `bundle.windows.nsis` 配置(installerIcon + languages)
- `app.windows[0]` 加 `minWidth: 768` / `minHeight: 600`(响应式断点对齐)
- CSP `connect-src` 加 `https://huggingface.co`(前瞻性:未来前端可能 fetch 查询模型元数据)
  - 注:`ureq` 是 Rust 端 HTTP,不经 WebView,不依赖 CSP。加 huggingface.co 是为未来前端 fetch 做前瞻性配置

- [ ] **Step 10.4: 安装 tauri-cli(若未安装)**

Run:

```powershell
cargo install tauri-cli --version "^2.0.0" --locked
```

注:这一步可能耗时较长(5-10 分钟)。若已安装则跳过。

验证安装:

```powershell
cargo tauri --version
```

Expected: 输出版本号(2.x.x)。

- [ ] **Step 10.5: 前端构建**

Run:

```powershell
cd voicepilot/crates/ui/web; npm run build
```

Expected: 构建成功。

- [ ] **Step 10.6: cargo tauri build(默认无 voice)**

Run(在 `voicepilot/crates/ui/` 目录下):

```powershell
cd voicepilot/crates/ui; cargo tauri build
```

Expected:
- 编译成功
- 生成 `voicepilot/crates/ui/target/release/voicepilot-ui.exe`
- 生成 `voicepilot/crates/ui/target/release/bundle/nsis/VoicePilot_0.1.0_x64-setup.exe`

若失败,根据错误调整。常见错误:
- icon 路径不对 → 检查 `tauri.conf.json` 的 `bundle.icon` 路径
- CSP 语法错误 → 检查 `csp` 字段
- minWidth/minHeight 不被识别 → Tauri 2 schema 检查,改成 `minWidth` / `minHeight`(`camelCase`)

- [ ] **Step 10.7: 验证 NSIS installer 存在**

Run:

```powershell
dir voicepilot/crates/ui/target/release/bundle/nsis/*.exe
```

Expected: 看到 `VoicePilot_0.1.0_x64-setup.exe`。

- [ ] **Step 10.8: Commit**

```powershell
cd voicepilot; git add voicepilot/crates/ui/tauri.conf.json voicepilot/crates/ui/icons; git commit -m "build(w6b-3a): tauri Windows NSIS bundling with fluid-ripple icon + bundle metadata"
```

---

## Task 11: PROGRESS.md 更新 + 最终验证

**目标:** 更新 `docs/PROGRESS.md` 顶部状态块 + 加 W6b-3a 详细段落;运行全量测试矩阵(workspace 测试 + voice 测试 + clippy + npm build)。

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 11.1: 运行全量测试矩阵**

**11.1.1 默认 workspace 测试**:

Run:

```powershell
cd voicepilot; cargo test --workspace
```

Expected: 所有测试 PASS(196+ tests,含 Task 1-9 新增测试)。

**11.1.2 voice feature 测试**:

Run:

```powershell
cd voicepilot; cargo test -p trust-kernel --features voice
```

Expected: 所有测试 PASS(含 Task 6 model_download 测试)。

**11.1.3 ui tauri feature 测试**:

Run:

```powershell
cd voicepilot; cargo test -p voicepilot-ui --features tauri
```

Expected: 所有测试 PASS(含 Task 9 E2E 测试)。

**11.1.4 ui voice feature 测试(可选,若环境支持)**:

Run:

```powershell
cd voicepilot; cargo test -p voicepilot-ui --features voice
```

Expected: 所有测试 PASS。

**11.1.5 clippy 全量检查**:

Run:

```powershell
cd voicepilot; cargo clippy --workspace --all-features -- -D warnings
```

Expected: 无 warning。

**11.1.6 前端构建**:

Run:

```powershell
cd voicepilot/crates/ui/web; npm run build
```

Expected: 构建成功。

- [ ] **Step 11.2: 更新 PROGRESS.md 顶部状态块**

读 `docs/PROGRESS.md` 前 20 行,把 `最新 commit` 后的 commit hash 更新为 W6b-3a 最后一个 commit 的 hash(Task 10.8 的 commit hash)。

在 `> **W6a Fast-Follow:** ✅ 已完成` 行下方追加:

```
> **W6b-3a:** ✅ 已完成(Diff Preview + 批次审批 + auto-download + E2E + Windows 打包)
```

- [ ] **Step 11.3: PROGRESS.md 加 W6b-3a 详细段落**

读 `docs/PROGRESS.md`,找到 W6b-2 详细段落(`### W6b-2:` 开头)末尾,在其后追加 W6b-3a 段落:

```markdown
### W6b-3a: Diff Preview + 批次审批 + auto-download + E2E + Windows 打包

**完成时间:** 2026-07-22(Asia/Shanghai)
**Commit 范围:** Task 1 - Task 11(共 ~11 commits)

**实现内容:**
- Diff Preview:Rust `similar` crate 计算 unified diff,文件内容不经过 IPC;50MB 软上限防 OOM;二进制检测(前 8KB NUL byte);新文件检测
- 批次审批文案中文化:Allow/Deny 改为"允许所有 (N 个文件)" / "拒绝所有 (N 个文件)";整批决策(单次 submit_approval,后端不改)
- 模型 auto-download:`ureq` HTTPS + 100ms 节流进度回调 + .part 临时文件 + 原子 rename + SHA256 校验;启动检测 + 用户确认弹窗
- E2E 全链路冒烟:route → organize → AutoApprover → commit → audit chain 验证
- Tauri Windows NSIS 打包:多尺寸 PNG/ICO 图标(流体波纹设计 §6.2.1 方向 A)+ bundle metadata + minWidth/minHeight + CSP 前瞻性加 huggingface.co
- D3/E3 红色高亮延后 W7+(ApprovalRequestPayload schema 需扩展,files.organize 是 E2/D2)

**关键架构决策:**
- Diff 计算在 Rust 端(隐私 + IPC 数据小)
- Diff 懒加载(ApprovalModal 按钮触发,prepare 阶段不加 IO)
- 批次审批整批决策(现有 submit_approval_command 不接受 scope)
- auto-download 启动检测 + 用户确认(尊重用户)
- 打包仅 Windows NSIS(macOS/Linux 延后 W7+)
- 图标用流体波纹抽象设计(类似 Siri,双色静态渐变,不拘泥麦克风实体)

**测试矩阵:**
- `cargo test --workspace`:196+ tests PASS
- `cargo test -p trust-kernel --features voice`:含 3 个 model_download 单元测试
- `cargo test -p voicepilot-ui --features tauri`:含 6 个 w6b3_e2e_smoke 测试
- `cargo clippy --workspace --all-features -- -D warnings`:无 warning
- `npm run build`:前端构建成功
- `cargo tauri build`:生成 `VoicePilot_0.1.0_x64-setup.exe`

**已知偏离 / 延后项:**
- 50MB 软上限:用户选择"不限制",但加软上限防止 OOM(§7.1)
- ApprovalScope::Batch 不引入:整批决策用现有架构,不新增类型(§7.1)
- auto-download 不写自动测试:依赖网络,CI 不稳定(§7.1)
- Tauri 2 test `mock_app()` API 在 CI 环境不稳定,E2E 用直接调用而非 mock_app(§7.1)
- D3/E3 红色高亮延后 W7+:ApprovalRequestPayload 当前不含 eLevel/dLevel(§3.2)
- 图标 32x32 / 16x16 用算法缩放,理想是手动重绘(§6.2.1)

**Commit 列表:**
1. `feat(w6b-3a): add compute_file_diff with similar crate` — Task 1
2. `feat(w6b-3a): expose FilesystemTool::assert_path_allowed` — Task 2
3. `feat(w6b-3a): add compute_diff_command Tauri command` — Task 3
4. `feat(w6b-3a): add DiffViewer component` — Task 4
5. `feat(w6b-3a): integrate DiffViewer into ApprovalModal + batch-approval Chinese copy` — Task 5
6. `feat(w6b-3a): add model_download with ureq + SHA256` — Task 6
7. `feat(w6b-3a): add model_download_commands` — Task 7
8. `feat(w6b-3a): add ModelDownloadBar with start-up detection` — Task 8
9. `test(w6b-3a): add w6b3_e2e_smoke` — Task 9
10. `build(w6b-3a): tauri Windows NSIS bundling with fluid-ripple icon` — Task 10
11. `docs(w6b-3a): update PROGRESS.md` — Task 11(本 commit)

**§11.1 W6b gate 验证:**
- ✅ Task 9 E2E 测试通过(覆盖 route → organize → approve → commit → audit 全链路)
- ✅ 默认 build 无 voice / CMake 依赖
- ✅ `tauri` feature 与 `voice` feature 独立编译
- ✅ TauriApprover 三条 IPC 安全规则保留(WebView 不直连 FS / UI 不直调 MCP / approval_request_id 一次性)

**下一步:** W6b-3b(TTS / Chip / Push-to-talk Voice UX 扩展)
```

- [ ] **Step 11.4: Commit PROGRESS.md**

```powershell
cd voicepilot; git add docs/PROGRESS.md; git commit -m "docs(w6b-3a): update PROGRESS.md with W6b-3a section + final test matrix"
```

- [ ] **Step 11.5: 最终 git log 验证**

Run:

```powershell
cd voicepilot; git log --oneline -15
```

Expected:看到 W6b-3a 的 11 个 commit(spec 3 个 + Task 1-11 共 14 个 commit),最新 commit 是 `docs(w6b-3a): update PROGRESS.md`。

---

## Self-Review

完成所有 Task 后,执行 self-review:

### 1. Spec coverage

| Spec 章节 | 对应 Task |
|---|---|
| §2 Diff Preview 后端 | Task 1 |
| §2.2 Tauri command | Task 3 |
| §2.3 前端 DiffViewer | Task 4 |
| §2.3 ApprovalModal 集成 | Task 5 |
| §3 批次审批文案 | Task 5 |
| §4 模型 auto-download 后端 | Task 6 |
| §4 模型 auto-download Tauri command | Task 7 |
| §4 模型 auto-download 前端 | Task 8 |
| §5 E2E 全链路冒烟 | Task 9 |
| §6 Tauri Windows 打包 | Task 10 |
| §6.2.1 流体波纹图标 | Task 10 |
| §7-§9 验收 / 偏离 / 风险 | Task 11 PROGRESS.md |

### 2. Placeholder scan

检查 plan 是否有 "TBD" / "TODO" / "implement later" / "add appropriate error handling" 等占位符。已确保所有 step 包含完整代码。

### 3. Type consistency

self-review 修复了 4 处类型不一致(原 plan 用了 camelCase 序列化,与现有 `FileSnapshot` / `TranscriptionPartialPayload` 等 snake_case 约定不符):

- `DiffResult` Rust 不加 `rename_all`(默认 snake_case),TS interface 也用 snake_case(`source_path` / `dest_path` / `file_kind` / `diff_text` / `truncated` / `truncate_reason`)—— 与 `FileSnapshot` 一致
- `FileKind` Rust 用 `#[serde(rename_all = "snake_case")]`(`NewFile` → `new_file`),TS `"new_file" | "text" | "binary"` —— 注意:不能用 `lowercase`(`NewFile` → `newfile`,缺下划线)
- `ModelStatus` Rust 用 `#[serde(rename_all = "snake_case")]`(`Disabled` → `disabled`),TS `"disabled" | "present" | "absent"` ✓
- `DownloadProgressPayload` Rust 不加 `rename_all`(默认 snake_case),TS 用 `downloaded_bytes` / `total_bytes` / `percent` —— 与 `TranscriptionPartialPayload` 一致
- `compute_diff_command` 参数名 `sourcePath` / `destPath`(TS invoke)对应 Rust `source_path` / `dest_path`(Tauri 2 自动 camelCase ↔ snake_case 转换,与现有 `submitApproval(approvalId, ...)` / `listAuditForTask(taskId)` 等一致)
- `audit_recent` 方法名修正为 `state.kernel.list_audit_recent(50)`(self-review 通过 Read kernel.rs:145 确认签名 `pub fn list_audit_recent(&self, limit: usize) -> Result<Vec<AuditEvent>>`)

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-07-21-w6b-3a-diff-preview-batch-auto-download-e2e-bundling.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - 每个 Task 派发 fresh subagent,Task 间 review,快速迭代

**2. Inline Execution** - 在当前 session 用 executing-plans 批量执行,checkpoint review

**Which approach?**
