---
name: web.page_screenshot
description: 网页截图：用 Playwright 对自动化会话的当前页面截图。契约：无参数（截当前页面）；要截取指定网页时由 LLM 拆解为 web.page_navigate → 本技能两步。需要 Playwright MCP server 已导入且已标记信任。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: playwright
      tool_name: browser_take_screenshot
---

# web.page_screenshot

Playwright MCP `browser_take_screenshot` 的一等公民绑定。无参数调用（截图当前自动化页面）。

链式用法（LLM 拆解）：web.page_navigate → 本技能。
