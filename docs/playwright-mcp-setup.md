# Playwright MCP 配置指南

> **用途:** VoicePilot 的 `research.save_markdown` 与 `form.prepare` 两个浏览器自动化 Skill 通过 [Playwright MCP](https://github.com/microsoft/playwright-mcp) server 驱动 Chromium,完成网页抓取与表单填充。本指南介绍用户侧依赖安装与故障排查。

---

## 1. 前置依赖

### 1.1 Node.js ≥ 18

Playwright MCP 是 Node.js 包,需先安装 Node.js 18 或更高版本。

**Windows(推荐 nvm-windows):**

```powershell
# 通过 nvm-windows 安装(推荐)
nvm install 18
nvm use 18
node --version  # 应输出 v18.x.x 或更高

# 或直接下载 LTS 安装包:https://nodejs.org/dist/latest-v18.x/
```

> VoicePilot 永久 Windows-only(用户决策 2026-07-26),不提供 macOS / Linux 安装说明。

### 1.2 网络访问

首次调用 `research.save_markdown` 或 `form.prepare` 时,`npx -y @playwright/mcp@latest` 会从 npm registry 下载 Playwright MCP 包及其依赖(约 50MB)。后续调用走本地缓存,无需网络。

如部署在离线环境,可预先在有网机器上运行 `npm pack @playwright/mcp@latest` 获取 tarball,拷贝到目标机器后用 `npm install -g` 安装。

### 1.3 浏览器二进制

Playwright MCP 默认使用 Chromium。首次运行时会自动下载(约 150MB,需网络)。如需指定浏览器:

```json
// 在 mcp_servers.args 中追加 --browser 参数
["-y", "@playwright/mcp@latest", "--browser", "firefox"]
```

可选值:`chromium`(默认)/ `firefox` / `webkit`。

---

## 2. 配置确认

VoicePilot 在内核启动时自动向 `mcp_servers` 表插入默认 `playwright` 记录,无需手动配置:

| 字段 | 默认值 |
|---|---|
| `server_id` | `playwright` |
| `command` | `npx` |
| `args` | `["-y", "@playwright/mcp@latest"]` |
| `enabled` | `true` |
| `transport` | `stdio` |

可在 **Trust Center → MCP Servers** 面板查看与切换启用状态。如需自定义浏览器或代理,直接编辑该记录的 `args` 字段。

---

## 3. 验证安装

### 3.1 命令行验证

```powershell
# 验证 Node.js
node --version  # ≥ v18

# 验证 npx(随 Node.js 安装)
npx --version

# 预下载 Playwright MCP(可选,避免首次调用超时)
npx -y @playwright/mcp@latest --help
```

### 3.2 VoicePilot 内验证

启用 LLM(可选)→ 在 Main View 输入:

> "把这个网页存为 Markdown:<https://example.com>"

应触发 `research.save_markdown` Skill,审批 Allow 后,文件保存到 `Documents/research-<uuid>.md`。

### 3.3 自动化测试验证

```powershell
cd d:\voicepilot\voicepilot
# Mock MCP 测试(无需 Node.js)
cargo test -p trust-kernel --test w7_plan5_mcp_playwright_smoke

# 真实 Playwright MCP 测试(需 Node.js ≥ 18 + 网络)
cargo test -p trust-kernel --test w7_plan5_mcp_playwright_smoke -- --ignored
```

---

## 4. 故障排查

### 4.1 `mcp_playwright_unavailable` 错误

**症状:** Skill 执行返回 `error_code = "mcp_playwright_unavailable"`,审计日志显示 MCP server 启动失败。

**排查步骤:**

1. **检查 Node.js 安装:**
   ```powershell
   node --version
   ```
   如命令未找到,安装 Node.js ≥ 18(见 §1.1)。

2. **检查 npx 可用性:**
   ```powershell
   npx --version
   ```
   npx 随 Node.js 自动安装。如缺失,重装 Node.js LTS。

3. **手动启动 MCP server:**
   ```powershell
   npx -y @playwright/mcp@latest
   ```
   应输出 JSON-RPC initialize 握手日志。如报 `EPERM` 或 `EACCES`,检查 npm 全局缓存权限。

4. **检查 VoicePilot MCP 记录:**
   在 Trust Center → MCP Servers 确认 `playwright` 记录 `enabled = true`。

5. **检查审计日志:**
   查看 `audit_logs` 表中 `step_id` 对应的失败记录,`error_message` 字段含具体 spawn 失败原因。

### 4.2 浏览器二进制下载失败

**症状:** `npx @playwright/mcp@latest` 启动后报 `browser not found` 或下载超时。

**解决:**

```powershell
# 手动下载 Chromium
npx playwright install chromium

# 或设置镜像(中国大陆推荐)
$env:PLAYWRIGHT_DOWNLOAD_HOST="https://npmmirror.com/mirrors/playwright"
npx playwright install chromium
```

### 4.3 首次调用超时

**症状:** 首次执行 `research.save_markdown` 等待 30s+ 后超时,后续调用正常。

**原因:** `npx` 首次需下载 Playwright MCP 包 + 依赖,耗时较长。

**解决:**

```powershell
# 预热缓存
npx -y @playwright/mcp@latest --help
# 等待下载完成后再使用 VoicePilot
```

或在 VoicePilot 启动前手动安装到全局:

```powershell
npm install -g @playwright/mcp@latest
```

然后将 `mcp_servers.args` 改为 `["@playwright/mcp@latest"]`(去掉 `-y`,因为已全局安装)。

### 4.4 表单填充不完整

**症状:** `form.prepare` 执行后,部分表单字段未填充。

**排查:**

1. 检查 `fields` 参数中的 CSS selector 是否正确(可在浏览器 DevTools 中 `document.querySelector(...)` 验证)
2. 检查页面是否需要滚动到表单元素(Playwright MCP 的 `fill` 会自动滚动,但某些 lazy-load 组件可能需要先 `click` 触发)
3. 查看审计日志中的 `tool_calls` 记录,确认每个 `fill` 调用的返回值

**注意:** `form.prepare` 故意不点击 submit 按钮。用户审批后需手动点击提交,或通过后续 `playwright.click` 调用(暂未实现,留待 W7+ Skill 编排)。

---

## 5. 安全注意事项

- **`allowed_paths` 为空:** Playwright MCP 不直接访问文件系统,所有写操作走 VoicePilot 的 `filesystem.write` 工具(受 `assert_path_allowed` 白名单约束)
- **审批 PerStep:** 每个 `research.save_markdown` / `form.prepare` 调用都需用户审批,EffectManifest 显示完整 URL + 字段数
- **不点击 submit:** `form.prepare` 仅填充表单,不触发提交。用户可审阅填充结果后再决定是否手动提交
- **审计日志完整:** 所有 MCP 工具调用(navigate / snapshot / eval / fill)均记录到 `audit_logs` 表,含 `preconditions_hash` 防篡改

---

## 6. 卸载

如需移除 Playwright MCP:

1. 在 Trust Center → MCP Servers 禁用 `playwright` 记录(将 `enabled` 设为 false)
2. (可选)清理 npm 缓存:
   ```powershell
   npm cache clean --force
   npx clear-npx-cache  # 如有该工具
   ```
3. (可选)删除浏览器二进制:
   ```powershell
   npx playwright uninstall --all
   ```

VoicePilot 内核启动时仍会插入默认 `playwright` 记录(幂等),但 `enabled = false` 的记录不会被调用。
