# W6a Fast-Follow 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 W6a 遗留的 3 个 fast-follow 项:ApprovalModal 卸载冗余 IPC、响应式布局(窄窗口汉堡菜单)、CSP 加固。

**Architecture:** 三项独立改动,无交叉依赖。Task 1 改 ApprovalModal.tsx(加 `useRef` 短路);Task 2 改 App.tsx + KillSwitchBar.tsx + styles.css(加 `isNarrow` state + `sidebarOpen` state + `@media` + overlay);Task 3 改 tauri.conf.json(CSP 追加 2 个 `none` 指令)。Task 4 最终验证 + commit PROGRESS.md。

**Tech Stack:** React 18 + TypeScript + Vite + Tauri 2 + CSS @media

**Spec:** [2026-07-21-w6a-fast-follow-design.md](file:///d:/voicepilot/docs/superpowers/specs/2026-07-21-w6a-fast-follow-design.md)

---

## File Structure

| 文件 | 改动类型 | 责任 |
|---|---|---|
| `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` | 修改 | 加 `submittedRef` 短路 cleanup effect |
| `voicepilot/crates/ui/web/src/App.tsx` | 修改 | 加 `isNarrow` + `sidebarOpen` state + resize 监听 + overlay backdrop + 传 props |
| `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx` | 修改 | 接收 `isNarrow` + `onToggleSidebar` props,窄窗口显示汉堡按钮 |
| `voicepilot/crates/ui/web/src/styles.css` | 修改 | 加 `.sidebar-toggle`/`.sidebar-backdrop` + `@media (max-width: 767px)` |
| `voicepilot/crates/ui/tauri.conf.json` | 修改 | CSP 追加 `object-src 'none'; frame-ancestors 'none'` |
| `docs/PROGRESS.md` | 修改 | 追加 W6a Fast-Follow 完成记录 |

---

## Task 1: ApprovalModal `submittedRef` 短路

**Files:**
- Modify: `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx`

- [ ] **Step 1: 修改 ApprovalModal.tsx —— 加 `useRef` import + `submittedRef` 短路**

将 `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` 完整替换为:

```tsx
import { useEffect, useRef, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // submittedRef 短路:decide() 成功后置 true,cleanup effect 跳过冗余 deny(W6a Fast-Follow)
  const submittedRef = useRef(false);
  const { approval_request_id, manifest } = payload;

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
              <span className="value">{manifest.sources.length}</span>
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
              </tr>
            </thead>
            <tbody>
              {manifest.sources.map((s) => (
                <tr key={s.canonical_path}>
                  <td className="path">{s.canonical_path}</td>
                  <td>{s.size}</td>
                  <td>{s.sha256.slice(0, 16)}…</td>
                </tr>
              ))}
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
            Deny
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}
```

**关键改动点:**
- L1: `import { useEffect, useRef, useState }` — 追加 `useRef`
- L12: `const submittedRef = useRef(false);` — 新增 ref
- L19: `submittedRef.current = true;` — `decide()` 成功后置 true(在 `onDismiss()` 前)
- L44-46: cleanup effect 加 `if (submittedRef.current) return;` 短路

- [ ] **Step 2: 构建前端验证**

Run:
```powershell
cd d:\voicepilot\voicepilot\crates\ui\web; npm.cmd run build
```
Expected: `vite build` 成功,无 TS 错误。`dist/` 重新生成。

- [ ] **Step 3: 提交**

```powershell
cd d:\voicepilot; git add voicepilot/crates/ui/web/src/components/ApprovalModal.tsx voicepilot/crates/ui/web/dist; git commit -m "fix(w6a-fast-follow): ApprovalModal submittedRef short-circuits redundant deny IPC"
```

---

## Task 2: 响应式布局(窄窗口汉堡菜单)

**Files:**
- Modify: `voicepilot/crates/ui/web/src/App.tsx`
- Modify: `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx`
- Modify: `voicepilot/crates/ui/web/src/styles.css`

- [ ] **Step 1: 修改 App.tsx —— 加 isNarrow + sidebarOpen state + resize 监听 + overlay**

将 `voicepilot/crates/ui/web/src/App.tsx` 完整替换为:

```tsx
import { useEffect, useState } from "react";
import { onApprovalRequest } from "./api";
import type { ApprovalRequestPayload, View } from "./types";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { SettingsView } from "./components/SettingsView";
import { AuditViewerView } from "./components/AuditViewerView";
import { TrustCenterView } from "./components/TrustCenterView";
import { SkillsManagerView } from "./components/SkillsManagerView";
import { KillSwitchBar } from "./components/KillSwitchBar";

const NAV_ITEMS: { view: View; label: string }[] = [
  { view: "main", label: "Main Chat" },
  { view: "settings", label: "Settings" },
  { view: "audit", label: "Audit Viewer" },
  { view: "trust", label: "Trust Center" },
  { view: "skills", label: "Skills Manager" },
];

const NARROW_BREAKPOINT = 768;

export function App(): JSX.Element {
  const [view, setView] = useState<View>("main");
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);
  // 响应式布局(W6a Fast-Follow):窄窗口隐藏 sidebar,改为 overlay
  const [isNarrow, setIsNarrow] = useState<boolean>(window.innerWidth < NARROW_BREAKPOINT);
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(false);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => setApproval(payload));
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 监听窗口尺寸变化(W6a Fast-Follow 响应式)
  useEffect(() => {
    const onResize = () => {
      setIsNarrow(window.innerWidth < NARROW_BREAKPOINT);
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
    };
  }, []);

  // 从窄变宽时自动关闭 overlay(避免宽屏残留 open state)
  useEffect(() => {
    if (!isNarrow) {
      setSidebarOpen(false);
    }
  }, [isNarrow]);

  const handleNavClick = (nextView: View): void => {
    setView(nextView);
    setSidebarOpen(false);
  };

  return (
    <div className="app-root">
      <KillSwitchBar
        isNarrow={isNarrow}
        onToggleSidebar={() => setSidebarOpen((o) => !o)}
      />
      <div className="app-body">
        <nav
          className={`sidebar ${isNarrow ? "narrow" : ""} ${sidebarOpen ? "open" : ""}`}
          aria-label="主导航"
          role={isNarrow && sidebarOpen ? "dialog" : undefined}
          aria-modal={isNarrow && sidebarOpen ? "true" : undefined}
        >
          <h1 className="app-title">VoicePilot</h1>
          <ul className="nav-list" role="list">
            {NAV_ITEMS.map((item) => (
              <li key={item.view}>
                <button
                  type="button"
                  className={`nav-item ${view === item.view ? "active" : ""}`}
                  onClick={() => handleNavClick(item.view)}
                  aria-pressed={view === item.view}
                  aria-current={view === item.view ? "page" : undefined}
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        {isNarrow && sidebarOpen && (
          <div
            className="sidebar-backdrop"
            onClick={() => setSidebarOpen(false)}
            aria-hidden="true"
          />
        )}
        <main className="main-content">
          {view === "main" && <MainView />}
          {view === "settings" && <SettingsView />}
          {view === "audit" && <AuditViewerView />}
          {view === "trust" && <TrustCenterView />}
          {view === "skills" && <SkillsManagerView />}
        </main>
      </div>
      {approval && (
        <ApprovalModal
          payload={approval}
          onDismiss={() => setApproval(null)}
        />
      )}
    </div>
  );
}
```

**关键改动点:**
- L19-20: `NARROW_BREAKPOINT = 768` 常量 + `isNarrow`/`sidebarOpen` state
- L31-39: `useEffect` 监听 `resize` 更新 `isNarrow`
- L42-46: `useEffect` 在 `isNarrow` 变 false 时自动关闭 overlay
- L48-51: `handleNavClick` 点击导航项后关闭 overlay
- L57: `<KillSwitchBar isNarrow={...} onToggleSidebar={...} />` 传 props
- L60-64: sidebar className 动态拼接 + `role`/`aria-modal` 条件 prop
- L79-84: sidebar-backdrop overlay 遮罩

- [ ] **Step 2: 修改 KillSwitchBar.tsx —— 接收 props + 显示汉堡按钮**

将 `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx` 完整替换为:

```tsx
import { useState } from "react";
import { cancelVoice } from "../api";

interface Props {
  /** 窄窗口模式(< 768px)— 显示汉堡按钮(W6a Fast-Follow 响应式) */
  isNarrow: boolean;
  /** 切换 sidebar overlay 开关 */
  onToggleSidebar: () => void;
}

/**
 * KillSwitchBar —— W6b-2 Task 6 紧急停止条(issue #57)+ W6a Fast-Follow 响应式汉堡菜单。
 *
 * 顶部条带,提供全局 cancel 按钮触发 `cancel_voice_command`。
 * 窄窗口时左侧加汉堡按钮,点击展开 sidebar overlay。
 */
export function KillSwitchBar({ isNarrow, onToggleSidebar }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);

  const handleKill = (): void => {
    setBusy(true);
    cancelVoice()
      .catch(console.error)
      .finally(() => setBusy(false));
  };

  return (
    <div className="kill-switch-bar" role="banner" aria-label="紧急停止">
      {isNarrow && (
        <button
          type="button"
          className="sidebar-toggle"
          onClick={onToggleSidebar}
          aria-label="切换导航菜单"
        >
          ☰
        </button>
      )}
      <span className="kill-switch-label">⚠ 紧急停止</span>
      <button
        type="button"
        className="kill-switch-btn"
        onClick={handleKill}
        disabled={busy}
        aria-label="停止所有执行"
      >
        {busy ? "停止中…" : "停止所有"}
      </button>
    </div>
  );
}
```

**关键改动点:**
- L4-9: `Props` interface(`isNarrow` + `onToggleSidebar`)
- L17: 函数签名解构 props
- L31-40: `isNarrow` 时渲染汉堡按钮(`☰` + `aria-label`)

- [ ] **Step 3: 修改 styles.css —— 加响应式样式**

在 `voicepilot/crates/ui/web/src/styles.css` 末尾追加:

```css

/* W6a Fast-Follow:响应式布局(窄窗口汉堡菜单) */
.kill-switch-bar {
  justify-content: space-between;
}
.kill-switch-bar > .kill-switch-label {
  margin-left: auto;
}
.sidebar-toggle {
  background: transparent;
  color: #fef2f2;
  border: 1px solid #fca5a5;
  padding: 4px 10px;
  border-radius: 4px;
  cursor: pointer;
  font-family: "IBM Plex Mono", monospace;
  font-size: 14px;
  line-height: 1;
}
.sidebar-toggle:hover {
  background: #7f1d1d;
}
.sidebar-toggle:focus-visible {
  outline: 2px solid #fbbf24;
  outline-offset: 2px;
}
.sidebar-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 40;
  animation: fadeIn 0.15s ease-out;
}
@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

@media (max-width: 767px) {
  .sidebar {
    position: fixed;
    top: 0;
    left: 0;
    bottom: 0;
    transform: translateX(-100%);
    z-index: 50;
    transition: transform 0.2s ease-out;
    box-shadow: 4px 0 12px rgba(0, 0, 0, 0.4);
  }
  .sidebar.open {
    transform: translateX(0);
  }
  .sidebar.narrow:not(.open) {
    display: none;
  }
  .main-content {
    padding: 16px;
  }
}
```

**关键样式说明:**
- `.kill-switch-bar` `justify-content: space-between` — 左汉堡 + 右停止按钮
- `.kill-switch-bar > .kill-switch-label` `margin-left: auto` — 标签推到右侧(汉堡按钮独占左侧)
- `.sidebar-toggle` — 汉堡按钮样式(透明背景 + 红边)
- `.sidebar-backdrop` — overlay 遮罩(`position: fixed` + `z-index: 40`)
- `@media (max-width: 767px)` — 窄窗口 sidebar 改 `position: fixed` + `transform: translateX(-100%)` 隐藏,`.open` 时 `translateX(0)` 滑入

- [ ] **Step 4: 构建前端 + cargo check 验证**

Run:
```powershell
cd d:\voicepilot\voicepilot\crates\ui\web; npm.cmd run build
cd d:\voicepilot; cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
```
Expected: `vite build` 成功;`cargo check` Finished。

- [ ] **Step 5: clippy + 测试无回归**

Run:
```powershell
cd d:\voicepilot; cargo clippy --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --all-targets -- -D warnings
cd d:\voicepilot; cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
```
Expected: clippy 0 warnings;16 tests passed(W6a 12 + w6b2_smoke 4)。

- [ ] **Step 6: 提交**

```powershell
cd d:\voicepilot; git add voicepilot/crates/ui/web/src/App.tsx voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx voicepilot/crates/ui/web/src/styles.css voicepilot/crates/ui/web/dist; git commit -m "feat(w6a-fast-follow): responsive sidebar with hamburger menu for narrow windows (< 768px)"
```

---

## Task 3: CSP 加固

**Files:**
- Modify: `voicepilot/crates/ui/tauri.conf.json`

- [ ] **Step 1: 修改 tauri.conf.json —— CSP 追加 2 个 none 指令**

将 `voicepilot/crates/ui/tauri.conf.json` 中 `security.csp` 字段替换为:

```json
"csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'"
```

完整文件应为:
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
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.ico"]
  }
}
```

**改动:** `csp` 字段末尾追加 `; object-src 'none'; frame-ancestors 'none'`(其余字符不变)。

- [ ] **Step 2: cargo check 验证 schema 有效**

Run:
```powershell
cd d:\voicepilot; cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
```
Expected: Finished(配置变更不影响编译,但 Tauri 在 `tauri::generate_context!` 编译期会解析 tauri.conf.json,若 CSP 字符串违反 schema 会报错)。

- [ ] **Step 3: 提交**

```powershell
cd d:\voicepilot; git add voicepilot/crates/ui/tauri.conf.json; git commit -m "feat(w6a-fast-follow): harden CSP with object-src 'none' + frame-ancestors 'none' (anti-clickjacking)"
```

---

## Task 4: 最终验证 + PROGRESS.md 更新

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: 默认特性测试无回归**

Run:
```powershell
cd d:\voicepilot; cargo test --manifest-path voicepilot\Cargo.toml
```
Expected: 196 passed, 0 failed。

- [ ] **Step 2: tauri 特性测试**

Run:
```powershell
cd d:\voicepilot; cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
```
Expected: 16 passed(W6a 12 + w6b2_smoke 4)。

- [ ] **Step 3: voice 特性测试无回归**

Run:
```powershell
cd d:\voicepilot; $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"; cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice
```
Expected: 45 passed。

- [ ] **Step 4: clippy 三组合**

Run:
```powershell
cd d:\voicepilot; cargo clippy --manifest-path voicepilot\Cargo.toml --all-targets -- -D warnings
cd d:\voicepilot; cargo clippy --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --all-targets -- -D warnings
cd d:\voicepilot; $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"; $env:WHISPER_DONT_GENERATE_BINDINGS = "1"; cargo clippy --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice --all-targets -- -D warnings
```
Expected: 三组合均 0 warnings。

- [ ] **Step 5: 前端构建**

Run:
```powershell
cd d:\voicepilot\voicepilot\crates\ui\web; npm.cmd run build
```
Expected: `dist/` 生成。

- [ ] **Step 6: 更新 PROGRESS.md**

在 `docs/PROGRESS.md` 顶部状态块追加 W6a Fast-Follow 行(在 W6b-2 行之后):

```
> **W6a Fast-Follow:** ✅ 已完成(2026-07-21)— ApprovalModal submittedRef 短路 + 响应式汉堡菜单 + CSP 加固,详见 §二 W6a Fast-Follow 段落
```

更新 `最新 commit` 行为本次最后一个 commit hash(Task 4 的 commit)。

在 W6b-2 段落之后追加 W6a Fast-Follow 段落:

```markdown
### W6a Fast-Follow: ApprovalModal submittedRef + 响应式布局 + CSP 加固 (3 commits)

**实现内容:**
- ApprovalModal `submittedRef` 短路:cleanup effect 在已提交时跳过冗余 `submitApproval("deny")` IPC(保留 Esc/关窗 deny 语义)
- 响应式布局(窄窗口汉堡菜单):窗口 < 768px 时 sidebar 改 overlay,KillSwitchBar 左侧加汉堡按钮 + backdrop + 导航项点击关闭
- CSP 加固:tauri.conf.json `csp` 追加 `object-src 'none'; frame-ancestors 'none'`(防 clickjacking + 禁插件)

**新增/修改文件:**
```
voicepilot/crates/ui/web/src/
├── components/ApprovalModal.tsx   # +useRef + submittedRef 短路
├── components/KillSwitchBar.tsx   # +Props(isNarrow + onToggleSidebar)+ 汉堡按钮
├── App.tsx                        # +isNarrow/sidebarOpen state + resize 监听 + overlay backdrop
└── styles.css                     # +.sidebar-toggle/.sidebar-backdrop + @media (max-width: 767px)
voicepilot/crates/ui/tauri.conf.json  # +object-src 'none' + frame-ancestors 'none'
```

**W6a Fast-Follow commits (按时序,直接提交到 master):**
| Commit | 任务 |
|---|---|
| `<hash1>` | Task 1: ApprovalModal submittedRef short-circuits redundant deny IPC |
| `<hash2>` | Task 2: responsive sidebar with hamburger menu for narrow windows (< 768px) |
| `<hash3>` | Task 3: harden CSP with object-src 'none' + frame-ancestors 'none' |
| `<hash4>` | Task 4: docs PROGRESS.md - W6a Fast-Follow completion |

**核心架构决策:**
- **submittedRef vs useEffect 依赖数组**:`useRef` 不触发 re-render,仅在 cleanup 时读取,比 `useEffect [submitted]` 依赖更简单
- **CSS @media + JS isNarrow 双轨**:`@media` 处理视觉(transform/display),JS `isNarrow` 处理交互(汉堡按钮渲染 + overlay 逻辑)。单靠 `@media` 无法控制 React 渲染
- **sidebar overlay 用 fixed + transform**:`position: fixed` 脱离文档流,`transform: translateX(-100%)` 隐藏,`.open` 时 `translateX(0)` 滑入,`z-index: 50` + backdrop `z-index: 40`
- **CSP 追加而非重写**:保留原有指令,只追加两个 `none`,最小改动

**测试矩阵(W6a Fast-Follow 验证):**
| 命令 | feature | 结果 |
|---|---|---|
| `cargo test` | (default) | 196 passed, 0 failed |
| `cargo test -p voicepilot-ui --features tauri` | tauri | 16 passed (W6a 12 + w6b2_smoke 4) |
| `cargo test -p voicepilot-ui --features voice` | voice | 45 passed |
| `cargo clippy --all-targets -- -D warnings` | (default) | 0 warnings |
| `cargo clippy -p voicepilot-ui --features tauri -- -D warnings` | tauri | 0 warnings |
| `cargo clippy -p voicepilot-ui --features voice -- -D warnings` | voice | 0 warnings |
| `cargo check -p voicepilot-ui --features tauri` | tauri | Finished (CSP schema 有效) |
| `npm.cmd run build` | — | dist/ 生成 |

**手动验证(未自动化):**
- ApprovalModal 点 Allow 后 DevTools Network 观察 `submit_approval` 只调 1 次(非 2 次)
- 直接关闭窗口 → fire 1 次 deny
- 窗口拖窄到 < 768px → 显示汉堡按钮 + sidebar overlay + backdrop 关闭 + 导航项点击关闭
- 应用启动后 DevTools Console 无 CSP 违规
```

- [ ] **Step 7: 提交 PROGRESS.md**

```powershell
cd d:\voicepilot; git add docs/PROGRESS.md; git commit -m "docs(w6a-fast-follow): PROGRESS.md - 3 fast-follow fixes completion (submittedRef + responsive + CSP)"
```

- [ ] **Step 8: 验证 git log**

Run:
```powershell
cd d:\voicepilot; git log --oneline -n 6
```
Expected: 看到 Task 1-4 的 4 个 commit + 之前的 `9191d95` (W6b-2 docs) + `7a00cc8` (W6a Fast-Follow spec)。

---

## 自审清单

**1. 规格覆盖(spec §2.1/§2.2/§2.3):**
- ✅ §2.1 ApprovalModal submittedRef — Task 1
- ✅ §2.2 响应式布局 — Task 2(App.tsx + KillSwitchBar.tsx + styles.css)
- ✅ §2.3 CSP 加固 — Task 3
- ✅ §6 验证标准 — Task 4(默认/tauri/voice 测试 + clippy 三组合 + npm build + cargo check)
- ✅ §4 测试策略 — Task 4 验证矩阵 + 各 Task 内步骤(无自动化测试,因 web/ 无 vitest)

**2. 占位符扫描:**
- 无 "TBD"/"TODO"/"implement later"
- 所有步骤都有实际代码或命令
- Task 4 Step 6 的 commit hash 用 `<hash1>` 等占位 —— 执行时用实际 `git log` 取 hash 填入

**3. 类型一致性:**
- `Props { isNarrow: boolean; onToggleSidebar: () => void }` — Task 2 Step 2 定义,App.tsx 使用 ✅
- `NARROW_BREAKPOINT = 768` — App.tsx 定义,与 spec §2.2 阈值一致 ✅
- `submittedRef = useRef(false)` — Task 1 定义,与 spec §2.1 一致 ✅
- CSP 字符串 — Task 3 与 spec §2.3 完全一致 ✅

**4. 风险点:**
- ⚠️ Task 2 sidebar overlay 在 Tauri 窗口默认 1024×768 下不显示,需手动拖窄验证。无自动化测试。
- ⚠️ Task 1 submittedRef 短路在 jsdom 难模拟(web/ 无 vitest),仅手动 DevTools 验证。
- ⚠️ Task 3 CSP 变更不影响编译,但 Tauri runtime 会强制执行 CSP。若前端代码有 inline script 会违规 —— 当前 styles.css 用 `'unsafe-inline'` 已允许,Vite 构建无 inline script。

**5. 已知偏离(将记录在 PROGRESS.md):**
- 无自动化测试(web/ 无 vitest 配置,新增 vitest 超出 fast-follow 范围)
- 响应式仅 768px 单断点(无中间断点,spec §2.2 明确)

---

## 执行交接

Plan complete and saved to `docs/superpowers/plans/2026-07-21-w6a-fast-follow.md`.
