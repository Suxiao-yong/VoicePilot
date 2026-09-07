---
name: web.page_fill
description: 网页填表：用 Playwright 在已打开的自动化页面输入文本到表单字段。契约：需要先 web.page_navigate 打开页面；输入 JSON 必须含 element（字段描述）、ref（快照元素引用号）与 text（要输入的文本），ref 必须来自 web 快照。需要 Playwright MCP server 已导入且已标记信任。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: playwright
      tool_name: browser_type
---

# web.page_fill

Playwright MCP `browser_type` 的一等公民绑定。输入示例：`{"element": "用户名输入框", "ref": "e5", "text": "alice"}`。

链式用法（LLM 拆解）：web.page_navigate → 多次本技能填字段 → （提交用既有 form.submit 或 web.page_click）。
