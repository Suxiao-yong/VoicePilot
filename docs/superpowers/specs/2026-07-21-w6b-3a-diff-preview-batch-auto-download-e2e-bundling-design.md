# W6b-3a 设计:Diff Preview + 批次审批 + auto-download + E2E + 打包

> **承接:** W6a Fast-Follow(已完成 2026-07-21,3 commits `9d93264`/`d71169d`/`9056498`)
> **范围:** W6b-3 拆分为 2 个子项目,本文档为 W6b-3a(核心 3 项 + 批次审批 + auto-download);W6b-3b(§8.4 TTS / Chip / Push-to-talk Voice UX 扩展)另立 spec
> **规格版本:** V1.1.2
> **创建时间:** 2026-07-21 (Asia/Shanghai)

---

## 1. 总体架构

### 1.1 范围

W6b-3a 是 W6 收尾里程碑的第一子项目,包含 5 个功能子任务 + 1 个收尾,共 6 个 Task:

| Task | 内容 | 主要文件 | 估算 commit 数 |
|---|---|---|---|
| 1 | Diff Preview(Rust `similar` crate + 前端 DiffViewer 懒加载) | `crates/trust-kernel/src/tools/diff.rs`(新)+ `crates/ui/src/diff_commands.rs`(新)+ `crates/ui/web/src/components/DiffViewer.tsx`(新)+ ApprovalModal 集成 | 4-5 |
| 2 | 批次审批按钮文案(整批决策)+ D3/E3 红色高亮(延后 W7+) | `ApprovalModal.tsx`(改) | 1 |
| 3 | 模型 auto-download(启动检测 + 下载进度) | `crates/trust-kernel/src/voice/model_download.rs`(新)+ `crates/ui/src/model_download_commands.rs`(新)+ `crates/ui/web/src/components/ModelDownloadBar.tsx`(新)+ `App.tsx` 启动 hook | 3-4 |
| 4 | E2E 全链路冒烟 | `crates/ui/tests/w6b3_e2e_smoke.rs`(新) | 2 |
| 5 | Tauri Windows 打包 | `tauri.conf.json`(改)+ `icons/`(补)+ `npm run tauri build` 验证 | 2-3 |
| 6 | PROGRESS.md 更新 + 最终验证 | `docs/PROGRESS.md`(改) | 1 |

### 1.2 架构图

```
┌─────────────────────────────────────────────────────────────┐
│  W6b-3a: Diff Preview + 批次审批 + auto-download + E2E + 打包  │
└─────────────────────────────────────────────────────────────┘
                          │
   ┌──────────────────────┼──────────────────────┐
   ▼                      ▼                      ▼
[Rust 层]              [前端层]              [打包层]
  │                      │                      │
  │ 1. compute_file_diff │ 1. DiffViewer 组件   │ 1. tauri.conf.json
  │   (similar crate)    │   (懒加载按钮)       │   补全 bundle 字段
  │ 2. download_model    │ 2. ApprovalModal     │ 2. icon.png 512×512
  │   (ureq + sha256)    │   Allow All + 红色   │ 3. icon.ico 多尺寸
  │ 3. check_model       │   高亮               │ 4. NSIS .exe 验证
  │   _present           │ 3. ModelDownloadBar  │
  │                      │   (启动检测弹窗)     │
  │                      │ 4. is_voice_enabled  │
  │                      │   _command           │
   └──────────────────────┴──────────────────────┘
                          │
                          ▼
              [E2E 测试 w6b3_e2e_smoke.rs]
              voice → route → organize → TauriApprover
              → approval-request → submit_approval
              → commit → audit chain
                          │
                          ▼
              [PROGRESS.md 更新]
```

### 1.3 架构原则

- **功能隔离**:每个 Task 自包含,可独立 commit + 验证
- **TDD red-green-commit**:每个 Task 先写失败测试 → 实现 → 验证 → commit
- **feature gate 保留**:voice 相关命令继续 `#[cfg(feature = "voice")]`,打包时默认 build 不含 voice,CMake 不依赖
- **§11.1 W6b gate**:Task 4 E2E 通过即满足 gate

### 1.4 关键设计决策汇总

| # | 决策点 | 选择 | 理由 |
|---|---|---|---|
| 1 | Diff 计算位置 | Rust 端 `similar` crate | 隐私(文件内容不经过 IPC)+ IPC 数据小 + D3 脱敏在 Rust 层完成 |
| 2 | Diff 触发时机 | ApprovalModal 懒加载按钮 | prepare 阶段不加 IO 开销,大文件不拖慢 approval,用户选择性查看 |
| 3 | 大文件处理 | 不限制 + 50MB 软上限 OOM 安全网 | 用户选择不限制,但加软上限防止极端情况 OOM |
| 4 | 打包平台 | 仅 Windows NSIS installer | 开发环境是 Windows,最小可用交付;macOS/Linux 延后 W7+ |
| 5 | auto-download 触发 | App 启动检测 + 用户确认弹窗 | 尊重用户(不擅自下载)+ 体验好(无需手动到 Settings) |
| 6 | 批次审批实现 | 整批决策(单次 submit_approval,后端不改)+ 按钮文案中文化 | 现有架构一个 approval_id 对应一个 manifest,一次 decision 应用于整批;D3/E3 红色高亮延后 W7+(files.organize 是 E2/D2,实现是 dead code) |
| 7 | E2E 覆盖范围 | 完整链路(voice → route → organize → TauriApprover → commit → audit) | 覆盖 §11.1 W6b gate 全链路,需调研 Tauri 2 test 模块 |
| 8 | 执行顺序 | 功能 → E2E → 打包 | 打包最后,确保功能完整 |

---

## 2. Diff Preview 详细设计

### 2.1 后端 Rust 层

**新增模块**:`crates/trust-kernel/src/tools/diff.rs`

```rust
//! Diff Preview 工具(W6b-3a Task 1)。
//! 用 `similar` crate 计算 unified diff,Rust 端完成,文件内容不经过 IPC。

use std::fs;
use std::path::Path;
use similar::TextDiff;
use serde::{Deserialize, Serialize};

/// Diff 计算结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "lowercase")]
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
/// **路径约定**:`source_path` 和 `dest_path` 都是**完整文件路径**(非目录)。
/// files.organize 场景下 `manifest.destination` 是目录,前端调用时需拼接:
/// `destPath = manifest.destination + "/" + basename(source_path)`
///
/// - `source_path` 必须存在(读取内容)
/// - `dest_path` 可不存在(返回 FileKind::NewFile,diff 为全文件内容)
/// - 超过 50MB 返回 truncated=true,diff_text=None
/// - 二进制文件返回 FileKind::Binary,diff_text=None
pub fn compute_file_diff(source_path: &Path, dest_path: &Path) -> crate::error::Result<DiffResult> {
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
            truncate_reason: Some(format!("文件超过 50MB 软上限({} bytes)", source_size)),
        });
    }

    let source_content = fs::read(source_path)?;

    // 二进制检测:前 8KB 含 NUL byte
    let is_binary = source_content
        .iter()
        .take(8 * 1024)
        .any(|&b| b == 0);

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
```

**新增依赖**(`crates/trust-kernel/Cargo.toml`):
```toml
similar = "2"  # MIT/Apache-2.0 dual,纯 Rust 无 C 依赖
```

### 2.2 Tauri command 层

**新增文件**:`crates/ui/src/diff_commands.rs`

```rust
//! Diff Preview Tauri commands(W6b-3a Task 1)。
//! cfg-gated 在 tauri feature 下。

use std::path::PathBuf;
use tauri::State;
use trust_kernel::tools::diff::{compute_file_diff, DiffResult};
use trust_kernel::TrustKernel;

use crate::error::UiError;
use crate::state::AppState;

/// 计算单文件 diff。
///
/// 前端在 ApprovalModal 中点击"查看 Diff"按钮调用。
/// 路径必须通过 allowed_paths 白名单(由 FilesystemTool 强制)。
#[tauri::command]
pub fn compute_diff_command(
    state: State<'_, AppState>,
    source_path: String,
    dest_path: String,
) -> Result<DiffResult, String> {
    compute_diff_impl(state, &source_path, &dest_path).map_err(Into::into)
}

fn compute_diff_impl(
    state: State<'_, AppState>,
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

**注册到 `register_handlers`**(commands.rs):
```rust
crate::diff_commands::compute_diff_command,
```

**注意**:`FilesystemTool::assert_path_allowed` 是 W6b-3a 新增的公开方法。现有 `FilesystemTool` 已有 `allowed_paths` 字段(W3b 引入),但 `assert_path_allowed` 是 private。W6b-3a Task 1 需要将其改为 `pub`。

### 2.3 前端层

**新增组件**:`crates/ui/web/src/components/DiffViewer.tsx`

```tsx
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
    invoke<DiffResult>("compute_diff_command", {
      sourcePath,
      destPath,
    })
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
        <button type="button" onClick={onClose} aria-label="关闭 Diff">
          ×
        </button>
      </div>
      <div className="diff-paths">
        <div><strong>源:</strong> <code>{sourcePath}</code></div>
        <div><strong>目标:</strong> <code>{destPath}</code></div>
      </div>
      {loading && <p>加载中…</p>}
      {error && <p className="diff-error">错误:{error}</p>}
      {result && !loading && !error && <DiffContent result={result} />}
    </div>
  );
}

function DiffContent({ result }: { result: DiffResult }): JSX.Element {
  if (result.truncated) {
    return <p className="diff-truncated">⚠ {result.truncateReason}</p>;
  }
  if (result.fileKind === "binary") {
    return <p className="diff-binary">二进制文件,不展示 diff</p>;
  }
  if (result.fileKind === "new" && result.diffText) {
    return (
      <pre className="diff-text diff-new-file">
        {result.diffText}
      </pre>
    );
  }
  return <pre className="diff-text">{result.diffText ?? "(无差异)"}</pre>;
}
```

**ApprovalModal 集成**(改 `ApprovalModal.tsx`):

每个 source 行末加"查看 Diff"按钮,点击后展开 DiffViewer:

```tsx
const [expandedDiff, setExpandedDiff] = useState<string | null>(null);

// 在 manifest-table 的 sources.map 内,新增 Diff 按钮 + 折叠 DiffViewer
<tr key={s.canonical_path}>
  <td className="path">{s.canonical_path}</td>
  <td>{s.size}</td>
  <td>{s.sha256.slice(0, 16)}…</td>
  <td>
    <button
      type="button"
      className="diff-toggle-btn"
      onClick={() => setExpandedDiff(expandedDiff === s.canonical_path ? null : s.canonical_path)}
      aria-expanded={expandedDiff === s.canonical_path}
      aria-label={`查看 ${s.canonical_path} 的 Diff`}
    >
      {expandedDiff === s.canonical_path ? "收起" : "Diff"}
    </button>
  </td>
</tr>
{expandedDiff === s.canonical_path && (
  <tr>
    <td colSpan={4}>
      <DiffViewer
        sourcePath={s.canonical_path}
        destPath={`${manifest.destination}/${s.canonical_path.split(/[\\/]/).pop()}`}
        onClose={() => setExpandedDiff(null)}
      />
    </td>
  </tr>
)}
```

### 2.4 测试策略

| 测试 | 类型 | 文件 | feature |
|---|---|---|---|
| `compute_file_diff_new_file` | unit | `trust-kernel/src/tools/diff.rs`(inline `#[cfg(test)]`) | default |
| `compute_file_diff_text_diff` | unit | 同上 | default |
| `compute_file_diff_binary_detection` | unit | 同上 | default |
| `compute_file_diff_truncated_50mb` | unit | 同上 | default |
| `compute_diff_command_allowed_path` | integration | `ui/tests/diff_commands_unit.rs`(新) | tauri |
| `compute_diff_command_blocked_path` | integration | 同上 | tauri |
| 前端 DiffViewer | 手动 | — | — |

---

## 3. 批次审批(整批决策)详细设计

### 3.1 后端:无改动

**关键澄清**(self-review 修正):现有 `submit_approval_command` 接收 `approval_id: String` + `decision: ApprovalDecision`,**不接受 scope 字段**。一个 `approval_request_id` 对应一个 `EffectManifest`(含多个 sources),一次 decision 应用于整个 manifest。

因此 "批次审批" 实际上是 "整批决策":用户点 "允许所有" → 一次 `submit_approval(allow)` → executor 对 manifest.sources 逐个执行 commit_move。**不需要前端循环调用**。

后端 `ApprovalRepo` schema 不动,executor 逻辑不动,`submit_approval_command` 签名不动。

**ApprovalScope::Batch 延后 W7+**(规格 §8.3 未明确定义 batch schema,本计划不引入)。

### 3.2 前端:ApprovalModal 按钮文案 + 行为澄清

**改 `ApprovalModal.tsx`**:

现有 ApprovalModal 已有 Allow / Deny 按钮(W6a 实现),W6b-3a Task 2 仅做以下改动:

1. **按钮文案中文化**:"Allow" → "允许所有","Deny" → "拒绝所有"(明确表示决策应用于整个 manifest)
2. **多文件提示**:manifest.sources.length > 1 时,按钮文案显示 "允许所有 (N 个文件)"
3. **保留 submittedRef 短路**(W6a Fast-Follow 已实现)

```tsx
const sourceCount = manifest.sources.length;

<div className="approval-actions" role="group" aria-label="审批操作">
  <button
    type="button"
    className="approval-btn allow"
    onClick={handleAllow}
    disabled={busy || submittedRef.current}
    aria-label={sourceCount > 1 ? `允许所有 ${sourceCount} 个文件` : "允许"}
  >
    {busy ? "处理中…" : sourceCount > 1 ? `允许所有 (${sourceCount} 个文件)` : "允许"}
  </button>
  <button
    type="button"
    className="approval-btn deny"
    onClick={handleDeny}
    disabled={busy || submittedRef.current}
    aria-label={sourceCount > 1 ? `拒绝所有 ${sourceCount} 个文件` : "拒绝"}
  >
    {sourceCount > 1 ? `拒绝所有 (${sourceCount} 个文件)` : "拒绝"}
  </button>
</div>
```

**handleAllow / handleDeny 沿用 W6a 现有实现**(单次 submit_approval 调用,不循环):

```tsx
const handleAllow = async (): Promise<void> => {
  setBusy(true);
  try {
    await submitApproval({
      approvalRequestId: payload.approval_request_id,  // 注意:snake_case
      decision: "allow",
    });
    submittedRef.current = true;
    onDismiss();
  } catch (e) {
    setError(`允许失败:${String(e)}`);
  } finally {
    setBusy(false);
  }
};
```

### 3.3 D3/E3 红色高亮:延后 W7+

**self-review 决定**:D3/E3 红色高亮延后 W7+,原因:

1. `ApprovalRequestPayload` 当前不含 `e_level` / `d_level` 字段(仅 `approval_request_id` + `manifest`)
2. 现有 files.organize Skill 是 E2/D2(非高风险),红色高亮永远不触发 = dead code
3. 实现红色高亮需要扩展 `ApprovalRequestPayload` schema + 改 `TauriApprover::prompt` 签名传入 risk_level,影响范围大
4. YAGNI 原则:W7+ 引入 D3/E3 Skill 时再实现

**W7+ 实现预案**(记录在此供后续参考):
- `ApprovalRequestPayload` 新增 `e_level: u8` + `d_level: u8`
- `TauriApprover::prompt` 从 SkillManifest 读取 risk_label 注入 payload
- 前端 `isHighRisk = e_level >= 3 || d_level >= 3`
- 红色高亮样式(.approval-btn.high-risk / .risk-badge)在 W7+ Task 中新增

### 3.4 测试策略

**不新增测试**(self-review 修正):

- `w6a_e2e_smoke.rs` 已覆盖 `end_to_end_organize_files_with_auto_approver_full_pipeline`(allow)+ `end_to_end_deny_cancels_commit`(deny)
- Task 2 仅改 UI 文案,逻辑无变化,无需新增集成测试
- 前端文案改动仅手动验证

| 验证项 | 类型 | 文件 |
|---|---|---|
| 多文件按钮文案显示 "(N 个文件)" | manual | ApprovalModal.tsx |
| 单文件按钮文案显示 "允许" / "拒绝" | manual | ApprovalModal.tsx |
| submittedRef 短路仍工作 | 已有测试 | w6a_e2e_smoke.rs |

---

## 4. 模型 auto-download(issue #46)详细设计

### 4.1 后端:下载管理器

**新增模块**:`crates/trust-kernel/src/voice/model_download.rs`

```rust
//! 模型 auto-download 管理器(W6b-3a Task 3,issue #46)。
//! cfg-gated 在 voice feature 下。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 模型元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,           // "ggml-tiny.bin"
    pub download_url: String,   // HTTPS URL
    pub expected_sha256: String,
    pub size_bytes: u64,        // ~75MB for tiny
}

/// 默认模型:ggml-tiny.bin(Whisper.cpp tiny 模型)。
pub fn default_model_info() -> ModelInfo {
    ModelInfo {
        name: "ggml-tiny.bin".to_string(),
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin".to_string(),
        expected_sha256: "bd577a113a864445d4c17714cdf623e26a0481f9ded9aa81004c888527036e0c".to_string(),
        size_bytes: 77_700_000, // ~75MB
    }
}

/// 模型目录:~/.voicepilot/models/(跨平台)。
pub fn models_dir() -> crate::error::Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| crate::error::KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "无法定位用户 home 目录",
        )))?;
    let dir = home.join(".voicepilot").join("models");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 检查模型是否存在 + SHA256 校验通过。
pub fn check_model_present() -> crate::error::Result<bool> {
    let info = default_model_info();
    let path = models_dir()?.join(&info.name);
    if !path.exists() {
        return Ok(false);
    }
    // 校验 sha256
    let actual = crate::tools::fs_snapshot::sha256_of_file(&path)?;
    Ok(actual == info.expected_sha256)
}

/// 下载进度回调。
pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub bytes_per_sec: f64,
}

/// 下载模型(阻塞,前端通过 Tauri event 接收进度)。
///
/// - 写入临时文件 .part,完成后原子 rename
/// - 下载完成后校验 sha256,失败则删除并返回错误
/// - 通过 progress_cb 回调上报进度(每 100ms 节流)
pub fn download_model(progress_cb: ProgressCallback) -> crate::error::Result<PathBuf> {
    let info = default_model_info();
    let dest_dir = models_dir()?;
    let dest_path = dest_dir.join(&info.name);
    let temp_path = dest_dir.join(format!("{}.part", info.name));

    // 已存在且校验通过,直接返回
    if dest_path.exists() {
        let actual = crate::tools::fs_snapshot::sha256_of_file(&dest_path)?;
        if actual == info.expected_sha256 {
            return Ok(dest_path);
        }
        fs::remove_file(&dest_path)?;
    }

    // HTTPS 下载(用 ureq 简单同步 HTTP 客户端,避免引入 tokio 复杂度)
    let response = ureq::get(&info.download_url)
        .timeout(Duration::from_secs(300))
        .call()
        .map_err(|e| crate::error::KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("下载失败:{}", e),
        )))?;

    let total = response.header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(info.size_bytes);

    let mut file = fs::File::create(&temp_path)?;
    let mut reader = response.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let start = std::time::Instant::now();
    let mut last_report = std::time::Instant::now();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        let elapsed = start.elapsed().as_secs_f64();
        let bytes_per_sec = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
        let percent = if total > 0 { (downloaded as f64 / total as f64) * 100.0 } else { 0.0 };

        // 节流:每 100ms 上报一次
        if last_report.elapsed() >= Duration::from_millis(100) {
            progress_cb(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes: total,
                percent,
                bytes_per_sec,
            });
            last_report = std::time::Instant::now();
        }
    }
    file.flush()?;
    drop(file);

    // 下载完成,校验 sha256
    let actual = crate::tools::fs_snapshot::sha256_of_file(&temp_path)?;
    if actual != info.expected_sha256 {
        let _ = fs::remove_file(&temp_path);
        return Err(crate::error::KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("SHA256 校验失败:expected={}, actual={}", info.expected_sha256, actual),
        )));
    }

    // 原子 rename
    fs::rename(&temp_path, &dest_path)?;
    progress_cb(DownloadProgress {
        downloaded_bytes: total,
        total_bytes: total,
        percent: 100.0,
        bytes_per_sec: 0.0,
    });

    Ok(dest_path)
}
```

**新增依赖**:
- `crates/trust-kernel/Cargo.toml`:`ureq = { version = "2", features = ["tls"] }`(纯 Rust,无 C 依赖)
- `crates/trust-kernel/Cargo.toml`:`dirs = "5"`(跨平台 home 目录)

**注意**:`ureq` 和 `dirs` 仅在 voice feature 下启用(optional dep),避免 default build 引入 HTTPS 栈:

```toml
[dependencies]
ureq = { version = "2", features = ["tls"], optional = true }
dirs = { version = "5", optional = true }

[features]
voice = ["dep:whisper-rs", "dep:cpal", "dep:hound", "dep:ureq", "dep:dirs"]
```

### 4.2 Tauri command 层

**新增文件**:`crates/ui/src/model_download_commands.rs`

```rust
//! 模型下载 Tauri commands(W6b-3a Task 3)。
//! cfg-gated 在 voice feature 下(因依赖 WhisperEngine 模型)。

use std::sync::Arc;
use tauri::{AppHandle, State, Emitter};
use trust_kernel::voice::model_download::{
    check_model_present, download_model, DownloadProgress,
};

use crate::error::UiError;
use crate::state::AppState;

/// 检查模型是否存在(启动时调用)。
#[tauri::command]
pub fn check_model_command() -> Result<bool, String> {
    check_model_present().map_err(|e| UiError::Kernel(e.to_string()).into())
}

/// 下载模型(异步,通过事件上报进度)。
#[tauri::command]
pub async fn download_model_command(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<String, String> {
    let app_clone = app.clone();
    let progress_cb: Arc<dyn Fn(DownloadProgress) + Send + Sync> = Arc::new(move |p| {
        let _ = app_clone.emit("model-download-progress", p.clone());
    });

    // 在 blocking task 中执行(ureq 是同步)
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_model(progress_cb)
    })
    .await
    .map_err(|e| UiError::Kernel(format!("下载任务失败:{}", e)))?;

    match result {
        Ok(path) => Ok(path.to_string_lossy().into_owned()),
        Err(e) => Err(UiError::Kernel(e.to_string()).into()),
    }
}

/// 前端检测 voice feature 是否启用。
#[tauri::command]
pub fn is_voice_enabled_command() -> bool {
    cfg!(feature = "voice")
}
```

**注册位置**(commands.rs):
- `is_voice_enabled_command` → 注册到 `register_handlers`(无 voice feature 也注册,返回 `false`)+ `register_handlers_with_voice`(有 voice feature 时重复注册返回 `true`)
- `check_model_command` + `download_model_command` → 仅注册到 `register_handlers_with_voice`(依赖 voice feature 的 `trust_kernel::voice::model_download` 模块)

```rust
// register_handlers(tauri feature only)
pub fn register_handlers(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        // ... existing commands
        crate::model_download_commands::is_voice_enabled_command,  // 返回 false
    ])
}

// register_handlers_with_voice(voice feature on)
#[cfg(feature = "voice")]
pub fn register_handlers_with_voice(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        // ... existing commands
        crate::model_download_commands::is_voice_enabled_command,  // 返回 true
        crate::model_download_commands::check_model_command,
        crate::model_download_commands::download_model_command,
    ])
}
```

**注意**:同一 command 在两个 register_handlers 中重复注册会冲突。实际实现时需要:
- 方案 A:`is_voice_enabled_command` 用 `#[cfg(feature = "voice")]` 内部分支,只注册一次到 `register_handlers`,返回 `cfg!(feature = "voice")`
- 方案 B:两个 register_handlers 用不同的 command 名(`is_voice_enabled_v1` / `is_voice_enabled_v2`)

推荐方案 A(单一 command,内部 cfg 判断)。

### 4.3 前端:启动检测 + ModelDownloadBar

**新增组件**:`crates/ui/web/src/components/ModelDownloadBar.tsx`

```tsx
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DownloadProgress } from "../types";

interface State {
  status: "checking" | "present" | "absent" | "downloading" | "done" | "error";
  progress: DownloadProgress | null;
  error: string | null;
}

export function ModelDownloadBar(): JSX.Element | null {
  const [state, setState] = useState<State>({ status: "checking", progress: null, error: null });

  // 启动时检查
  useEffect(() => {
    let cancelled = false;
    invoke<boolean>("check_model_command")
      .then((present) => {
        if (!cancelled) {
          setState({ status: present ? "present" : "absent", progress: null, error: null });
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setState({ status: "error", progress: null, error: String(e) });
      });
    return () => { cancelled = true; };
  }, []);

  // 监听下载进度事件
  useEffect(() => {
    if (state.status !== "downloading") return;
    const unlisten = listen<DownloadProgress>("model-download-progress", (event) => {
      setState((prev) => ({ ...prev, progress: event.payload }));
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [state.status]);

  // present/done 不渲染
  if (state.status === "present" || state.status === "done") {
    return null;
  }

  const handleDownload = (): void => {
    setState({ status: "downloading", progress: null, error: null });
    invoke<string>("download_model_command")
      .then(() => {
        setState({ status: "done", progress: null, error: null });
      })
      .catch((e: unknown) => {
        setState({ status: "error", progress: null, error: String(e) });
      });
  };

  return (
    <div className="model-download-bar" role="region" aria-label="模型下载">
      {state.status === "checking" && <p>检查模型…</p>}
      {state.status === "absent" && (
        <>
          <span>语音模型未安装(ggml-tiny.bin ~75MB)</span>
          <button type="button" onClick={handleDownload} aria-label="下载模型">
            下载模型
          </button>
        </>
      )}
      {state.status === "downloading" && state.progress && (
        <div className="download-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{ width: `${state.progress.percent}%` }}
              role="progressbar"
              aria-valuenow={Math.round(state.progress.percent)}
              aria-valuemin={0}
              aria-valuemax={100}
            />
          </div>
          <span>
            {state.progress.percent.toFixed(1)}% ({(state.progress.downloadedBytes / 1024 / 1024).toFixed(1)} / {(state.progress.totalBytes / 1024 / 1024).toFixed(1)} MB)
          </span>
        </div>
      )}
      {state.status === "error" && (
        <>
          <span className="error-text">下载失败:{state.error}</span>
          <button type="button" onClick={handleDownload} aria-label="重试下载">
            重试
          </button>
        </>
      )}
    </div>
  );
}
```

**App.tsx 集成**:

```tsx
import { ModelDownloadBar } from "./components/ModelDownloadBar";

// 启动时检测 voice feature,决定是否渲染 ModelDownloadBar
const [voiceEnabled, setVoiceEnabled] = useState<boolean>(false);

useEffect(() => {
  invoke<boolean>("is_voice_enabled_command")
    .then(setVoiceEnabled)
    .catch(() => setVoiceEnabled(false));
}, []);

// 渲染(KillSwitchBar 下方)
{voiceEnabled && <ModelDownloadBar />}
```

### 4.4 测试策略

| 测试 | 类型 | 文件 | feature |
|---|---|---|---|
| `check_model_present_returns_false_when_absent` | unit | `trust-kernel/src/voice/model_download.rs`(inline) | voice |
| `check_model_present_returns_true_when_valid` | unit | 同上 | voice |
| `check_model_present_returns_false_when_sha_mismatch` | unit | 同上 | voice |
| `models_dir_creates_dir` | unit | 同上 | voice |
| `default_model_info_has_expected_fields` | unit | 同上 | voice |
| `check_model_command_absent` | integration | `ui/tests/model_download_commands_unit.rs`(新) | voice |
| `is_voice_enabled_command_returns_true_with_voice` | integration | 同上 | voice |
| `is_voice_enabled_command_returns_false_without_voice` | integration | 同上(用 `--features tauri` 跑) | tauri |
| 下载流程 | 手动 | — | — |

**注意**:`download_model` 涉及真实 HTTPS 下载,不写自动测试,仅手动验证。

---

## 5. E2E 全链路冒烟详细设计

### 5.1 测试范围

完整链路:`voice_listen (StubVoiceListen mock) → route_text_command → organize_files_command (内部用 TauriApprover) → 测试代码监听 approval-request 事件自动响应 → submit_approval_command → 验证 commit + audit chain`

### 5.2 测试文件

**新增**:`crates/ui/tests/w6b3_e2e_smoke.rs`

```rust
//! W6b-3a Task 4: E2E 全链路冒烟测试(§11.1 W6b gate)。
//! cfg-gated 在 voice feature 下(因依赖 voice_listen)。

use std::sync::Arc;
use tauri::{test::{mock_builder, mock_app, noop_assets}, Manager, Wry};
use voicepilot_ui::{AppState, register_handlers_with_voice};
use trust_kernel::{TrustKernel, voice::{VoiceListen, VoiceListenResult, Transcription}};

/// StubVoiceListen:mock voice_listen,返回固定转写文本。
struct StubVoiceListen {
    transcript: String,
}

impl VoiceListen for StubVoiceListen {
    fn listen(&self, _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) -> VoiceListenResult<Transcription> {
        Ok(Transcription {
            text: self.transcript.clone(),
            language: Some("zh".to_string()),
            segments: vec![],
        })
    }
}

#[test]
fn end_to_end_voice_route_approval_execute_full_pipeline() {
    // 1. 初始化 TrustKernel(in-memory)
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // 2. 注入 StubVoiceListen
    let stub = Arc::new(StubVoiceListen {
        transcript: "organize E:/test_data *.txt E:/sorted".to_string(),
    });

    // 3. 创建 mock Tauri app
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            // 注册所有 voice + base commands
            voicepilot_ui::commands::route_text_command,
            voicepilot_ui::commands::organize_files_command,
            voicepilot_ui::commands::submit_approval_command,
            voicepilot_ui::voice_commands::voice_listen_command,
            voicepilot_ui::voice_commands::cancel_voice_command,
        ])
        .setup(move |app| {
            // 注入 AppState
            let state = AppState::new(kernel.clone(), stub.clone());
            app.manage(state);
            Ok(())
        })
        .build(noop_assets())
        .expect("mock app build failed");

    // 4. 触发 voice_listen_command
    let listen_result: String = tauri::test::call_api(
        &app,
        vec!["voice_listen_command".to_string()],
    ).unwrap();

    // 5. 监听 approval-request 事件,自动响应 allow
    let app_clone = app.handle().clone();
    app.listen_global("approval-request", move |event| {
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        let approval_request_id = payload["approvalRequestId"].as_str().unwrap().to_string();
        // 自动 submit approval allow
        let _ = tauri::test::call_api(
            &app_clone,
            vec!["submit_approval_command".to_string(),
                 serde_json::json!({
                     "approvalRequestId": approval_request_id,
                     "decision": "allow"
                 }).to_string()],
        );
    });

    // 6. 等待 organize_files 完成
    // (用 channel/condvar 等待 commit 事件或轮询 step 状态)

    // 7. 验证 audit chain
    let events = kernel.list_recent_audit_events(10).unwrap();
    assert!(events.iter().any(|e| e.event_type == "step_committed"));
    assert!(events.iter().any(|e| e.event_type == "approval_recorded"));
}
```

**注意**:上述代码是设计草案,实际实现需要:
1. 调研 Tauri 2 `tauri::test` 模块的具体 API(`mock_builder` / `mock_app` / `call_api`)
2. 用 channel 同步 voice_listen 完成 + approval-request 事件 + organize_files commit
3. 处理 async 等待(`tokio::time::sleep` 或 condvar)

### 5.3 Tauri 2 test 模块调研

Tauri 2 提供 `tauri::test` 模块,支持无头测试:
- `mock_builder()` 创建 mock Builder
- `mock_app()` 创建 mock App
- `call_api()` 调用 invoke handler
- `noop_assets()` 空资源

**已知限制**:
- mock_app 不加载真实 WebView,无法测前端交互
- 事件系统可用,但需手动 `app.listen_global` + `app.emit_to`
- async command 需要 `tokio::test` macro

### 5.4 测试策略

| 测试 | 类型 | 文件 | feature |
|---|---|---|---|
| `end_to_end_voice_route_approval_execute_full_pipeline` | integration | `ui/tests/w6b3_e2e_smoke.rs`(新) | voice |
| `end_to_end_deny_cancels_full_pipeline` | integration | 同上 | voice |

---

## 6. Tauri Windows 打包详细设计

### 6.1 tauri.conf.json 配置补全

**改 `crates/ui/tauri.conf.json`**:

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
    "windows": [{
      "label": "main",
      "title": "VoicePilot",
      "width": 1024,
      "height": 768,
      "resizable": true,
      "minWidth": 768,
      "minHeight": 600
    }],
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
    "copyright": "Copyright © 2026 VoicePilot Contributors",
    "category": "DeveloperTool",
    "shortDescription": "Voice-driven developer assistant with Trust Kernel",
    "longDescription": "VoicePilot is a voice-driven developer assistant with a Rust-based Trust Kernel, deterministic Skills, and Tauri-based desktop UI.",
    "windows": {
      "nsis": {
        "installerIcon": "icons/icon.ico",
        "installMode": "perMachine",
        "languages": ["English", "SimplifiedChinese"],
        "displayLanguageSelector": true
      }
    }
  }
}
```

**关键改动**:
1. `bundle.targets` 从 `"all"` 改为 `["nsis"]`(仅 Windows installer)
2. `bundle.icon` 补齐多尺寸 PNG(Tauri 2 要求至少 32x32 + 128x128 + 128x128@2x + ico)
3. `bundle.copyright` / `category` / `shortDescription` / `longDescription` 补全
4. `bundle.windows.nsis` 配置 NSIS installer 细节
5. `app.windows[0]` 加 `minWidth` / `minHeight`(768x600,与响应式断点对齐)
6. `app.security.csp` 加 `https://huggingface.co`(**注释**:ureq 是 Rust 端 HTTP 客户端,不经 WebView,CSP 不影响。但加 huggingface.co 到 connect-src 是为了未来前端可能用 fetch 查询模型元数据/版本更新时不受 CSP 拦截。当前 Task 3 的 download_model 不依赖此 CSP 项,可保留可删除,推荐保留为前瞻性配置)

### 6.2 图标资源补齐

**新增文件**:
- `crates/ui/icons/32x32.png`
- `crates/ui/icons/128x128.png`
- `crates/ui/icons/128x128@2x.png`(256x256)
- `crates/ui/icons/icon.ico`(替换现有 16x16,改为多尺寸 ICO:16/32/48/64/128/256)

**生成方式**(用 ImageMagick 或在线工具):
```powershell
# 从一张 512x512 源图生成所有尺寸
magick convert source-512.png -resize 32x32 icons/32x32.png
magick convert source-512.png -resize 128x128 icons/128x128.png
magick convert source-512.png -resize 256x256 icons/128x128@2x.png
magick convert source-512.png -define icon:auto-resize=16,32,48,64,128,256 icons/icon.ico
```

**源图设计**:沿用 W6a Engineering Console 美学(深海军蓝 + 暖琥珀),VoicePilot 字母 V + 麦克风图标组合。

### 6.3 voice feature 打包权衡

**问题**:voice feature 启用时,bundle 需包含 whisper-rs 静态库(~10-20MB),且 build 时需要 CMake + MSVC + libclang。

**方案**:提供两种打包模式:
1. **默认打包**(无 voice):`npm run tauri build`(无 CMake 依赖,bundle ~15MB)
2. **voice 打包**:`npm run tauri build -- --features voice`(需 CMake + libclang,bundle ~30MB)

**文档**:在 `docs/PROGRESS.md` + README 说明两种模式差异。

### 6.4 验证步骤

**前置:安装 tauri-cli**

```powershell
# 方式 1:cargo install(全局)
cargo install tauri-cli --version "^2.0.0"

# 方式 2:使用 cargo-tauri 子命令(推荐,避免全局安装)
# 后续命令用 `cargo tauri build` 即可
```

**打包流程**:

```powershell
# 1. 前端构建
cd voicepilot\crates\ui\web
npm.cmd run build
cd ..  # 回到 crates/ui 目录

# 2. Tauri 打包(默认无 voice)
cargo tauri build
# 验证:生成 target/release/bundle/nsis/VoicePilot_0.1.0_x64-setup.exe

# 3. voice 打包(需 CMake + libclang,可选)
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:WHISPER_DONT_GENERATE_BINDINGS = "1"
cargo tauri build -- --features voice
# 验证:生成 voice-enabled bundle

# 4. 安装测试
# 手动运行 .exe installer,验证安装 + 启动 + 基础功能
# 注意:未签名 installer 在 Windows SmartScreen 会拦截,
# 用户需点击"更多信息 → 仍要运行"
```

### 6.5 测试策略

| 测试 | 类型 | 文件 | feature |
|---|---|---|---|
| `tauri_conf_json_validates` | manual | — | — |
| `default_bundle_builds_successfully` | manual | — | — |
| `voice_bundle_builds_successfully` | manual | — | voice |
| `installer_runs_on_clean_windows` | manual | — | — |

---

## 7. 已知偏离/延后项

### 7.1 W6b-3a 范围内偏离

| 项 | 偏离 | 理由 |
|---|---|---|
| Diff 大文件处理 | 用户选"不限制",但加 50MB 软上限 OOM 安全网 | 防止极端情况 OOM,实际 99% 文件不受影响 |
| ApprovalScope::Batch | 不引入,用前端循环 Single | 后端 schema 不动,简化实现;Batch schema 延后 W7+ |
| auto-download 测试 | 不写自动测试,仅手动验证 | 涉及真实 HTTPS 下载,CI 环境不稳定 |
| Tauri 2 test 模块 | 调研后实现,可能需要调整测试代码 | Tauri 2 test API 仍在演进 |

### 7.2 延后到 W6b-3b

- §8.4 TTS 语音反馈
- §8.4 Chip 修改
- §8.4 Push-to-talk

### 7.3 延后到 W7+

- ApprovalScope::Batch 后端 schema
- D3/E3 红色高亮(需 ApprovalRequestPayload 扩展 e_level + d_level 字段 + TauriApprover::prompt 签名改动)
- macOS / Linux 打包
- 代码签名(SmartScreen 拦截提示文档化)
- CSP nonce 方案(替代 style-src 'unsafe-inline')
- 多窗口拆分(Main/Approval/Audit/Settings 独立 Tauri window)
- §8.4 低置信下划线 / 高风险参数视觉确认

---

## 8. 验收标准(§11.1 W6b gate)

### 8.1 功能验收

- [ ] Task 1:Diff Preview 在 ApprovalModal 中可懒加载展示,支持文本/新文件/二进制/截断四种情况
- [ ] Task 2:ApprovalModal 按钮文案中文化("允许所有 (N 个文件)" / "拒绝所有 (N 个文件)"),单文件时显示 "允许" / "拒绝";D3/E3 红色高亮延后 W7+
- [ ] Task 3:App 启动检测模型缺失时弹 ModelDownloadBar,下载进度可见,完成后校验 SHA256
- [ ] Task 4:E2E 全链路测试通过(voice → route → organize → approval → commit → audit)
- [ ] Task 5:Tauri Windows installer 生成成功,可在 clean Windows 上安装运行

### 8.2 测试验收

- [ ] `cargo test`(default):196+ tests passing,0 failures
- [ ] `cargo test -p voicepilot-ui --features tauri`:16+ tests passing(新增 diff_commands 2;Task 2 不新增测试)
- [ ] `cargo test -p voicepilot-ui --features voice`:45+ tests passing(新增 model_download 5 + w6b3_e2e 2)
- [ ] `cargo test -p trust-kernel --features voice`:23+ tests passing(新增 model_download unit 5 + diff unit 4)
- [ ] `cargo clippy --all-targets -- -D warnings`:0 warnings(default + tauri + voice)
- [ ] `npm.cmd run build`:无 TS 错误
- [ ] `cargo tauri build`:生成 NSIS installer

### 8.3 文档验收

- [ ] `docs/PROGRESS.md` 更新 W6b-3a 完成状态 + 顶部状态块 + 详细段落
- [ ] commit 历史清晰(每个 Task 独立 commit)

---

## 9. 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| Tauri 2 test 模块 API 不稳定 | 中 | 中 | Task 4 优先调研,若不可用退回 mock IPC 测试 |
| similar crate 与现有 deps 冲突 | 低 | 低 | 纯 Rust crate,无 C 依赖,冲突概率低 |
| ureq HTTPS 在某些网络环境失败 | 中 | 中 | 加重试逻辑 + 错误提示 + 手动下载文档 |
| NSIS installer 在 Windows SmartScreen 拦截 | 高 | 低 | 文档提示用户"更多信息 → 仍要运行",代码签名延后 W7+ |
| 50MB 软上限误判 | 低 | 低 | 仅 Rust 层硬编码,实际 99% 文件 < 1MB |
| voice feature 打包体积过大 | 中 | 中 | 默认打包不含 voice,voice 打包单独文档说明 |
| 图标资源版权问题 | 低 | 中 | 用自有设计(V + 麦克风),不使用第三方素材 |

---

## 10. 后续工作

W6b-3a 完成后:
1. **W6b-3b**:§8.4 TTS / Chip / Push-to-talk Voice UX 扩展(独立 spec → plan → 实现)
2. **W7**:LLM Planner + 8 Skills
3. **W8**:Stronghold Encryption + Taint Tracking

---

**文档版本:** V1.0
**创建者:** Claude(GLM-5.2)+ 用户协作
**审批状态:** 待用户审核
