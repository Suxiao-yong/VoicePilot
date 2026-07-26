-- W7 Plan 5 Task 0: add command/args/env columns to mcp_servers table.
-- These columns store the spawn spec for external MCP servers (e.g.
-- command='npx', args='["-y","@playwright/mcp@latest"]', env='{}').
--
-- SQLite does NOT support `ALTER TABLE ADD COLUMN IF NOT EXISTS` directly.
-- Idempotency is enforced by the migration runner (db.rs), which catches
-- "duplicate column name" errors per-statement and treats them as no-ops.
ALTER TABLE mcp_servers ADD COLUMN command TEXT;
ALTER TABLE mcp_servers ADD COLUMN args TEXT;
ALTER TABLE mcp_servers ADD COLUMN env TEXT;
