---
name: mail.draft
description: 建邮件草稿：用 Composio MCP 在 Gmail 创建草稿（mail.draft，不发信）。契约：输入 JSON 必须含 recipient（收件人邮箱）、subject（主题）、body（正文）；草稿仅存 Gmail 草稿箱，发送由用户人工确认 —— 人在环底线（沿用 mail.compose 的 mailto 语义）。需要 composio MCP server 已在 Trust Center 导入并标记信任。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: composio
      tool_name: GMAIL_CREATE_EMAIL_DRAFT
---

# mail.draft

Composio MCP `GMAIL_CREATE_EMAIL_DRAFT` 的一等公民绑定。输入示例：`{"recipient": "a@b.com", "subject": "周报", "body": "..."}`。

安全底线：绝不直接发送；草稿留在 Gmail 草稿箱等用户确认。
