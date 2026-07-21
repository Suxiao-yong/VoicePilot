# W6a Fast-Follow 设计文档

**日期:** 2026-07-21 (Asia/Shanghai)
**范围:** W6b-3 子集 — W6a Fast-Follow 三项修复
**规格引用:** V1.1.2 §8.2 IPC 三规则(不变)+ §8.3 WCAG A a11y + 工程加固
**前置状态:** W6b-2 已完成(最新 commit `9191d95`)

---

## 1. 背景与动机

W6a Tauri UI Shell 完成时记录了 3 个 fast-follow 项(PROGRESS.md L755-757),W6b-1/W6b-2 期间未处理:

1. **ApprovalModal 卸载时冗余 IPC**:cleanup effect 总会 fire `submitApproval("deny")`,即使用户已点 Allow/Deny 成功提交。Rust 端因 approval_request_id 一次性语义会返回 false,但仍消耗一次 RPC + 日志噪声
2. **响应式布局缺失**:tauri.conf.json `width: 1024` 固定,窄窗口(< 768px)时 W6b-2 sidebar 200px + main-content 横向挤压,form-row 标签 120px + 输入框挤压,SettingsView/AuditViewerView 表格溢出
3. **CSP 不完整**:tauri.conf.json `csp` 缺 `object-src 'none'; frame-ancestors 'none'`,未防 clickjacking 与插件加载

---

## 2. 设计方案

### 2.1 ApprovalModal `submittedRef` 短路

**文件:** `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx`

**改动:**
- import `useRef`
- 新增 `const submittedRef = useRef(false)`
- `decide()` 成功后(在 `onDismiss()` 前)`submittedRef.current = true`
- cleanup effect 改为:
  ```ts
  return () => {
    if (submittedRef.current) return;
    submitApproval(approval_request_id, "deny").catch(() => {});
  };
  ```

**保留语义:**
- 用户点 Allow/Deny → `decide()` 成功 → `submittedRef=true` → `onDismiss()` → 卸载 → cleanup 跳过 deny(正确,已提交)
- 用户按 Esc → `onDismiss()` 直接调 → 卸载 → cleanup 仍 fire deny(正确,未提交)
- 用户关闭窗口 → 卸载 → cleanup 仍 fire deny(正确,未提交)

**测试:** web/ 下无 vitest 配置(仅 vite + tsc,见 `package.json`),不写自动化测试。**仅手动验证**:
- 启动应用 → 触发 approval modal → 点 Allow → DevTools Network 观察 `submit_approval` IPC 只调 1 次(非 2 次)
- 启动应用 → 触发 approval modal → 直接关闭窗口 → DevTools Network 观察 `submit_approval` IPC 调 1 次 deny

### 2.2 响应式布局(窄窗口汉堡菜单)

**文件:**
- `voicepilot/crates/ui/web/src/App.tsx`
- `voicepilot/crates/ui/web/src/components/KillSwitchBar.tsx`
- `voicepilot/crates/ui/web/src/styles.css`

**方案:** 窗口宽度 < 768px 时 sidebar 隐藏,顶部 KillSwitchBar 左侧加汉堡按钮,点击展开 sidebar 为 overlay。

**App.tsx 改动:**
- 新增 state:`const [isNarrow, setIsNarrow] = useState(window.innerWidth < 768)`
- 新增 state:`const [sidebarOpen, setSidebarOpen] = useState(false)`
- `useEffect` 监听 `resize`:`window.addEventListener("resize", () => setIsNarrow(window.innerWidth < 768))`,cleanup 移除监听
- 从窄变宽时自动关闭 overlay:`useEffect(() => { if (!isNarrow) setSidebarOpen(false); }, [isNarrow])`
- 传 props 给 KillSwitchBar:`<KillSwitchBar isNarrow={isNarrow} onToggleSidebar={() => setSidebarOpen(o => !o)} />`
- sidebar 加 className:`className={`sidebar ${isNarrow ? "narrow" : ""} ${sidebarOpen ? "open" : ""}`}`
- sidebar overlay backdrop:`{isNarrow && sidebarOpen && <div className="sidebar-backdrop" onClick={() => setSidebarOpen(false)} aria-hidden="true" />}`
- 导航项点击后关闭 overlay:`onClick={() => { setView(item.view); setSidebarOpen(false); }}`

**KillSwitchBar.tsx 改动:**
- 接收 props:`{ isNarrow: boolean; onToggleSidebar: () => void }`
- `isNarrow` 时在左侧显示汉堡按钮:
  ```tsx
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
  ```
- KillSwitchBar 容器 `justify-content` 从 `flex-end` 改为 `space-between`(左汉堡 + 右停止按钮)

**styles.css 改动:**
- `.kill-switch-bar` `justify-content: flex-end` → `space-between`
- 加 `.sidebar-toggle`(汉堡按钮样式)
- 加 `.sidebar-backdrop`(overlay 遮罩)
- 加 `@media (max-width: 767px)`:
  - `.sidebar` 默认 `transform: translateX(-100%); position: fixed; top: 0; left: 0; bottom: 0; z-index: 50;`
  - `.sidebar.open` `transform: translateX(0);`
  - `.sidebar.narrow:not(.open)` `display: none;`(双重保险)
- a11y:sidebar overlay 时加 `role="dialog" aria-modal="true"`(通过 React 条件 prop)

**测试:** 仅手动验证(npm run dev + Tauri 窗口拖窄到 < 768px,验证汉堡按钮 + overlay + backdrop 关闭 + 导航项点击关闭)。resize 事件 + Tauri 窗口尺寸在 jsdom 难模拟,不写自动化测试。

### 2.3 CSP 加固

**文件:** `voicepilot/crates/ui/tauri.conf.json`

**改动:** `csp` 字段追加 `object-src 'none'; frame-ancestors 'none'`:
```json
"csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; frame-ancestors 'none'"
```

**影响:**
- `object-src 'none'`:禁止 `<object>`/`<embed>`/`<applet>` 加载(VoicePilot 不用,无影响)
- `frame-ancestors 'none'`:禁止页面被嵌入 iframe(防 clickjacking;Tauri 桌面应用本不应被嵌,加固)

**测试:** `cargo check -p voicepilot-ui --features tauri` 编译验证(配置变更不影响编译,但确认 schema 有效)+ 手动验证应用正常启动 + DevTools Console 无 CSP 违规。

---

## 3. 架构决策

- **submittedRef vs useEffect 依赖数组**:`useRef` 比 `useEffect [submitted]` 依赖更简单,不引入 re-render。`submittedRef` 是 mutable ref,不触发渲染,只在 cleanup 时读取
- **响应式用 CSS @media + JS isNarrow 双轨**:`@media` 处理视觉(transform/display),JS `isNarrow` 处理交互(是否显示汉堡按钮 + overlay 逻辑)。单靠 `@media` 无法控制 React 渲染汉堡按钮
- **sidebar overlay 用 fixed + transform**:`position: fixed` 脱离文档流,`transform: translateX(-100%)` 隐藏,`.open` 时 `translateX(0)` 滑入。配合 `z-index: 50` + backdrop `z-index: 40`
- **CSP 追加而非重写**:保留原有 `default-src/img-src/script-src/style-src/font-src/connect-src`,只追加两个 `none` 指令,最小改动

---

## 4. 测试策略

| 项目 | 测试方式 | 验证内容 |
|---|---|---|
| submittedRef 短路 | 手动(DevTools Network) | 点 Allow 后 `submit_approval` 只调 1 次;直接关闭窗口 fire 1 次 deny |
| 响应式布局 | 手动(npm run dev + 拖窄窗口) | < 768px 显示汉堡;点击展开 overlay;backdrop/导航项点击关闭;> 768px 恢复常驻 |
| CSP 加固 | `cargo check` + 手动启动 | 编译通过;应用启动;DevTools 无 CSP 违规 |
| 无回归 | `cargo test --features tauri` + `npm run build` | W6a/W6b-1/W6b-2 测试全通过;前端构建成功 |

---

## 5. 不在范围(留待 W6b-3 后续或 W7+)

- **Diff Preview**:§8.3 Approval Modal Diff Preview(文件内容读取器)— 独立子任务,留 W6b-3 后续
- **模型 auto-download(issue #46)**:留 W6b-3 或 W7
- **Tauri 打包(Windows installer)**:留 W6b-3 后续
- **E2E 冒烟(§11.1 W6b gate)**:留 W6b-3 收尾

---

## 6. 验证标准(§11.1 W6a Fast-Follow gate)

- `cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri`:16 tests passed(无回归)
- `cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features voice`:45 tests passed(无回归)
- `cargo clippy --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri -- -D warnings`:0 warnings
- `npm run build`:成功
- `cargo check -p voicepilot-ui --features tauri`:通过(CSP 配置有效)
- 手动:窄窗口汉堡菜单 + overlay 正常;ApprovalModal 点 Allow 后无冗余 deny;DevTools 无 CSP 违规

---

## 7. 已知偏离

- 无。三项均为 W6a 已记录的 fast-follow 项,本设计是对原记录的直接实现。
