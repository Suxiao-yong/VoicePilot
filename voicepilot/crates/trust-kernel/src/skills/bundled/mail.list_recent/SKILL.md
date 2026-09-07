---
name: mail.list_recent
description: 读收件箱：用 Composio MCP 拉取 Gmail 最近邮件列表（mail.list_recent，只读）。契约：输入 JSON 可含 query（Gmail 搜索词，如 "is:unread"）与 max_results（≤10）；发信不走本技能。需要 composio MCP server 已在 Trust Center 导入并标记信任；OAuth 授权在 Composio 侧完成，token 不进本地 DB。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: composio
      tool_name: GMAIL_FETCH_EMAILS
---

# mail.list_recent

Composio MCP `GMAIL_FETCH_EMAILS` 的一等公民绑定。输入示例：`{"query": "is:unread", "max_results": 5}`。

范围锁死（计划 Phase D）：只读收件箱；发信 = mail.draft 建草稿 + 用户确认。
