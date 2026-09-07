---
name: web.page_navigate
description: 网页导航：用 Playwright 浏览器自动化会话打开指定 URL（不点击、不填表）。契约：输入 JSON 必须含 url（http/https 完整地址）；本技能只建立会话状态，后续网页点击/网页填表/网页截图由其他 web.page_* 技能接续。需要 Playwright MCP server 已导入且已标记信任。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: playwright
      tool_name: browser_navigate
---

# web.page_navigate

Playwright MCP `browser_navigate` 的一等公民绑定。输入示例：`{"url": "https://example.com"}`。

链式用法（LLM 拆解）：本技能 → web.page_screenshot / web.page_click / web.page_fill。
