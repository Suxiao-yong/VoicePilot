---
name: web.page_click
description: 网页点击：用 Playwright 在已打开的自动化页面点击元素。契约：需要先 web.page_navigate 打开页面并拿到快照；输入 JSON 必须含 element（元素描述文本）与 ref（快照中的元素引用号，如 "e3"），两者都来自 web 快照，不得凭空编造。需要 Playwright MCP server 已导入且已标记信任。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: playwright
      tool_name: browser_click
---

# web.page_click

Playwright MCP `browser_click` 的一等公民绑定。输入示例：`{"element": "登录按钮", "ref": "e12"}`。

链式用法（LLM 拆解）：web.page_navigate → （快照由会话状态提供）→ 本技能。
