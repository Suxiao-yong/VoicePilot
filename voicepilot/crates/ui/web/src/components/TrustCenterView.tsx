import { useEffect, useState } from "react";
import {
  exportMcpServers,
  importExternalMcp,
  importMcpServers,
  listMcpServers,
  registerMcpServer,
  removeMcpServer,
  scanExternalMcp,
  toggleMcpServer,
} from "../api";
import type { ExternalMcpScanResult, McpServer } from "../types";

const emptyForm = (): McpServer => ({
  server_id: "",
  name: "",
  version: "1.0.0",
  transport: "stdio",
  enabled: true,
  trusted: false,
  protocol_version: "2025-11-25",
  allowed_origins: null,
  allowed_paths: null,
  command: null,
  args: null,
  env: null,
});

// 3-6:提交前校验 JSON 字段形状(后端解析失败只会回显错误,前端就地拦截更友好)
function jsonArrayOk(v: string | null): boolean {
  if (v === null || v.trim() === "") return true;
  try {
    return Array.isArray(JSON.parse(v));
  } catch {
    return false;
  }
}
function jsonObjectOk(v: string | null): boolean {
  if (v === null || v.trim() === "") return true;
  try {
    const p = JSON.parse(v);
    return p !== null && typeof p === "object" && !Array.isArray(p);
  } catch {
    return false;
  }
}

const setField =
  (key: keyof McpServer, value: string) =>
  (form: McpServer): McpServer => ({
    ...form,
    [key]: value.trim() === "" ? null : value,
  });

const setBool =
  (key: "enabled" | "trusted") =>
  (form: McpServer): McpServer => ({
    ...form,
    [key]: !form[key],
  });

export function TrustCenterView(): JSX.Element {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [form, setForm] = useState<McpServer>(emptyForm);
  // 标准 JSON 导入/导出(2026-08-24 统一)
  const [importText, setImportText] = useState<string>("");
  const [importNotice, setImportNotice] = useState<string | null>(null);
  const [exportText, setExportText] = useState<string | null>(null);
  const [busyJson, setBusyJson] = useState(false);
  const [externalMcp, setExternalMcp] = useState<ExternalMcpScanResult | null>(
    null,
  );
  const [scanningExternal, setScanningExternal] = useState(false);

  const refresh = (): void => {
    setLoading(true);
    listMcpServers()
      .then((s) => {
        setServers(s);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  };

  useEffect(refresh, []);

  const handleToggle = (serverId: string, enabled: boolean): void => {
    toggleMcpServer(serverId, enabled)
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const handleRemove = (serverId: string): void => {
    // 3-6:移除 MCP server 是破坏性操作,原生确认即可(YAGNI 不引对话框库)
    if (
      !window.confirm(
        `移除 MCP server「${serverId}」？仍被 User Skill 引用的 server 会被后端拒绝。`,
      )
    ) {
      return;
    }
    removeMcpServer(serverId)
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const handleRegister = (): void => {
    // 3-6:JSON 字段就地校验,错误直接标红不提交
    if (!jsonArrayOk(form.args)) {
      setError(
        '参数(args)必须是合法 JSON 字符串数组,如 ["-y","@playwright/mcp@latest"]',
      );
      return;
    }
    if (!jsonObjectOk(form.env)) {
      setError('环境变量(env)必须是合法 JSON 对象,如 {"TOKEN":"…"}');
      return;
    }
    if (!jsonArrayOk(form.allowed_paths)) {
      setError('allowed_paths 必须是合法 JSON 字符串数组,如 ["D:/readonly"]');
      return;
    }
    if (!jsonArrayOk(form.allowed_origins)) {
      setError("allowed_origins 必须是合法 JSON 字符串数组");
      return;
    }
    registerMcpServer(form)
      .then(() => {
        setForm(emptyForm());
        refresh();
      })
      .catch((e) => setError(String(e)));
  };

  // 标准 JSON 导入/导出(2026-08-24 统一:与 Claude/Cursor/VS Code 互通)
  const handleImportJson = (): void => {
    if (!importText.trim()) {
      setImportNotice(null);
      setError("请先粘贴标准 mcpServers JSON");
      return;
    }
    setBusyJson(true);
    setImportNotice(null);
    setError(null);
    importMcpServers(importText)
      .then((r) => {
        setImportNotice(
          r.errors.length === 0
            ? `已导入 ${r.imported} 个 MCP server（导入默认未信任,标记后可被 Skill 执行）`
            : `已导入 ${r.imported} 个,${r.errors.length} 个失败:` +
                r.errors.map((e) => ` ${e.name}(${e.error})`).join(";"),
        );
        setImportText("");
        refresh();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusyJson(false));
  };

  const handleExportJson = (): void => {
    setBusyJson(true);
    setError(null);
    exportMcpServers()
      .then((json) => setExportText(json))
      .catch((e) => setError(String(e)))
      .finally(() => setBusyJson(false));
  };

  const handleCopyExport = (): void => {
    if (exportText === null) return;
    void navigator.clipboard
      ?.writeText(exportText)
      .then(() => setExportText(null))
      .catch(() => setExportText(null));
  };

  const handleScanExternal = async (): Promise<void> => {
    setError(null);
    setImportNotice(null);
    setScanningExternal(true);
    try {
      setExternalMcp(await scanExternalMcp());
    } catch (e) {
      setError(String(e));
    } finally {
      setScanningExternal(false);
    }
  };

  const handleImportExternal = async (
    source_file: string,
    format: string,
    server_id: string,
  ): Promise<void> => {
    setError(null);
    setImportNotice(null);
    try {
      await importExternalMcp(source_file, format, server_id);
      setImportNotice(
        `已导入 ${server_id}（可见、默认未信任，去列表标记信任后可被执行）`,
      );
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section aria-labelledby="trust-heading">
      <header className="view-head">
        <h1 id="trust-heading" className="view-title">
          <span className="view-kicker">Trust</span>
          信任中心
        </h1>
        <p className="view-desc">
          管理 MCP Server 启用状态。停用后该 Server 不会被 Action Gateway 调用。
        </p>
      </header>

      {loading && (
        <div role="status" aria-live="polite">
          加载 MCP Server 列表…
        </div>
      )}
      {error && (
        <div className="alert alert-error" role="alert">
          <span className="alert-icon">⨯</span>
          <span>{error}</span>
        </div>
      )}

      {/* 标准 JSON 导入/导出(2026-08-24 统一:与 Claude Code / Cursor / VS Code 互通) */}
      <section aria-labelledby="json-compat-heading" className="card card-gap">
        <div className="card-head">
          <div>
            <h2 id="json-compat-heading" className="card-title">
              <span className="tick" aria-hidden="true" />
              标准 MCP 配置(JSON)
            </h2>
            <p className="card-desc">
              粘贴 Claude / Cursor / Windsurf 的
              <code className="mono"> mcpServers</code> 或 VS Code 的
              <code className="mono"> servers</code>{" "}
              配置直接导入。导入默认未信任,标记后才能被 Skill 执行。
            </p>
          </div>
          <div className="composer-meta tight">
            <button
              type="button"
              className="btn"
              onClick={handleExportJson}
              disabled={busyJson}
            >
              {busyJson ? "导出中…" : "导出为标准 JSON"}
            </button>
          </div>
        </div>
        <textarea
          className="field-input mono"
          rows={5}
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          placeholder={
            '{\n  "mcpServers": {\n    "filesystem": { "command": "npx", "args": ["-y", "…"] }\n  }\n}'
          }
          aria-label="标准 mcpServers JSON"
        />
        <div className="composer-meta tight">
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleImportJson}
            disabled={busyJson}
          >
            导入 MCP server
          </button>
        </div>
        {importNotice && (
          <div className="alert alert-info" role="status" aria-live="polite">
            <span className="alert-icon">✓</span>
            <span>{importNotice}</span>
          </div>
        )}
        {exportText !== null && (
          <div className="field field-gap">
            <label className="field-label" htmlFor="mcp-export-text">
              导出结果(复制到 Claude / Cursor / VS Code)
            </label>
            <textarea
              id="mcp-export-text"
              className="field-input mono"
              rows={8}
              readOnly
              value={exportText}
            />
            <button type="button" className="btn" onClick={handleCopyExport}>
              复制到剪贴板
            </button>
          </div>
        )}
      </section>

      {/* 外部发现：全局第三方 MCP 配置（只读扫描，导入默认未信任） */}
      <section aria-labelledby="external-mcp-heading" className="card card-gap">
        <div className="card-head">
          <div>
            <h2 id="external-mcp-heading" className="card-title">
              <span className="tick" aria-hidden="true" />
              外部发现 MCP
            </h2>
            <p className="card-desc">
              扫描 Claude Desktop / Cursor / VS Code 的 MCP
              配置。导入后可见、默认未信任，去列表标记信任后可被执行。
            </p>
          </div>
          <div className="composer-meta tight">
            <button
              type="button"
              className="btn"
              onClick={() => void handleScanExternal()}
              disabled={scanningExternal}
              aria-busy={scanningExternal}
              aria-label="扫描全局第三方 MCP 配置（只读）"
            >
              {scanningExternal ? "扫描中…" : "扫描外部 MCP"}
            </button>
          </div>
        </div>
        {externalMcp !== null && (
          <div className="table-wrap">
            {externalMcp.skipped.length > 0 && (
              <p className="result-sub" role="status">
                跳过 {externalMcp.skipped.length} 个不受支持条目：
                {externalMcp.skipped
                  .map((s) => `${s.server_id || "（空 id）"}（${s.reason}）`)
                  .join("；")}
              </p>
            )}
            <table className="data-table" aria-label="外部发现 MCP 列表">
              <thead>
                <tr>
                  <th scope="col">Server ID</th>
                  <th scope="col">名称</th>
                  <th scope="col">来源</th>
                  <th scope="col">命令</th>
                  <th scope="col">状态</th>
                  <th scope="col">操作</th>
                </tr>
              </thead>
              <tbody>
                {externalMcp.hits.map((hit) => {
                  const installed = servers.some(
                    (s) => s.server_id === hit.server_id,
                  );
                  return (
                    <tr key={`${hit.format}:${hit.server_id}`}>
                      <td className="mono">{hit.server_id}</td>
                      <td>{hit.name}</td>
                      <td>
                        {/* Phase A 去重：同 server 同内容多文件合并为一个候选，
                            来源列表全部展示；单来源时沿用 format pill。 */}
                        {hit.sources.length > 1 ? (
                          <span className="pill">
                            {hit.sources.length} 处：{hit.sources.join("\n")}
                          </span>
                        ) : (
                          <span className="pill">{hit.format}</span>
                        )}
                      </td>
                      <td className="mono cell-hash">{hit.command}</td>
                      <td>
                        {installed ? (
                          <span className="pill pill-on">已导入</span>
                        ) : (
                          <span className="pill">未导入</span>
                        )}
                      </td>
                      <td>
                        <button
                          type="button"
                          className="btn btn-sm"
                          disabled={installed}
                          onClick={() =>
                            void handleImportExternal(
                              hit.source_file,
                              hit.format,
                              hit.server_id,
                            )
                          }
                          aria-label={`导入外部 MCP ${hit.server_id}`}
                        >
                          导入
                        </button>
                      </td>
                    </tr>
                  );
                })}
                {externalMcp.hits.length === 0 && (
                  <tr>
                    <td colSpan={6} className="empty">
                      未发现外部 MCP 配置
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section aria-labelledby="register-heading" className="card card-gap">
        <h2 id="register-heading" className="card-title">
          注册 MCP Plugin
        </h2>
        <div className="field-row">
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-id">
              Server ID *
            </label>
            <input
              id="mcp-server-id"
              className="field-input mono"
              type="text"
              value={form.server_id}
              onChange={(e) => setForm(setField("server_id", e.target.value))}
              placeholder="my-readonly-server"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-name">
              名称 *
            </label>
            <input
              id="mcp-server-name"
              className="field-input"
              type="text"
              value={form.name}
              onChange={(e) => setForm(setField("name", e.target.value))}
              placeholder="My Read-only MCP"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-version">
              版本 *
            </label>
            <input
              id="mcp-server-version"
              className="field-input mono"
              type="text"
              value={form.version}
              onChange={(e) => setForm(setField("version", e.target.value))}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-transport">
              传输协议 *
            </label>
            <select
              id="mcp-server-transport"
              className="field-select mono"
              value={form.transport}
              onChange={(e) => setForm(setField("transport", e.target.value))}
            >
              <option value="stdio">stdio</option>
            </select>
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-protocol">
              协议版本
            </label>
            <input
              id="mcp-server-protocol"
              className="field-input mono"
              type="text"
              value={form.protocol_version ?? ""}
              onChange={(e) =>
                setForm(setField("protocol_version", e.target.value))
              }
              placeholder="2025-11-25"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-command">
              命令 *
            </label>
            <input
              id="mcp-server-command"
              className="field-input mono"
              type="text"
              value={form.command ?? ""}
              onChange={(e) => setForm(setField("command", e.target.value))}
              placeholder="npx"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-args">
              参数 (JSON 字符串数组)
            </label>
            <input
              id="mcp-server-args"
              className="field-input mono"
              type="text"
              value={form.args ?? ""}
              onChange={(e) => setForm(setField("args", e.target.value))}
              placeholder={JSON.stringify(["-y", "@playwright/mcp@latest"])}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-env">
              环境变量 (JSON 对象，值不写入审计/日志)
            </label>
            {/* 3-6:token 等敏感值明文展示太裸,改密文输入(值仍原样发送) */}
            <input
              id="mcp-server-env"
              className="field-input mono"
              type="password"
              autoComplete="off"
              value={form.env ?? ""}
              onChange={(e) => setForm(setField("env", e.target.value))}
              placeholder={JSON.stringify({ TOKEN: "…" })}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-paths">
              allowed_paths (JSON 字符串数组)
            </label>
            <input
              id="mcp-server-paths"
              className="field-input mono"
              type="text"
              value={form.allowed_paths ?? ""}
              onChange={(e) =>
                setForm(setField("allowed_paths", e.target.value))
              }
              placeholder={JSON.stringify(["D:/readonly"])}
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="mcp-server-origins">
              allowed_origins (JSON 字符串数组)
            </label>
            <input
              id="mcp-server-origins"
              className="field-input mono"
              type="text"
              value={form.allowed_origins ?? ""}
              onChange={(e) =>
                setForm(setField("allowed_origins", e.target.value))
              }
            />
          </div>
          <label className="field-check" htmlFor="mcp-server-trusted">
            <input
              id="mcp-server-trusted"
              type="checkbox"
              checked={form.trusted}
              onChange={() => setForm(setBool("trusted"))}
            />
            标记为可信（可被 User Skill 执行）
          </label>
          <div className="field">
            <button
              type="button"
              className="btn btn-primary"
              onClick={handleRegister}
            >
              注册插件
            </button>
          </div>
        </div>
      </section>

      <div className="table-wrap">
        <table className="data-table" aria-label="MCP Server 列表">
          <thead>
            <tr>
              <th scope="col">Server ID</th>
              <th scope="col">名称</th>
              <th scope="col">版本</th>
              <th scope="col">传输</th>
              <th scope="col">命令</th>
              <th scope="col">可信</th>
              <th scope="col">协议</th>
              <th scope="col">allowed_paths</th>
              <th scope="col">状态</th>
              <th scope="col">操作</th>
            </tr>
          </thead>
          <tbody>
            {servers.map((s) => (
              <tr key={s.server_id}>
                <td className="mono">{s.server_id}</td>
                <td>{s.name}</td>
                <td className="mono">{s.version}</td>
                <td className="mono">{s.transport}</td>
                <td className="mono">{s.command ?? "—"}</td>
                <td>
                  {s.trusted ? (
                    <span className="pill pill-on">可信</span>
                  ) : (
                    <span className="pill pill-off">未信任</span>
                  )}
                </td>
                <td className="mono">{s.protocol_version ?? "—"}</td>
                <td className="mono cell-hash">{s.allowed_paths ?? "—"}</td>
                <td>
                  <span
                    className={`pill ${s.enabled ? "pill-on" : "pill-off"}`}
                  >
                    {s.enabled ? "已启用" : "已停用"}
                  </span>
                </td>
                <td>
                  <button
                    type="button"
                    className={`btn btn-sm ${s.enabled ? "" : "btn-primary"}`}
                    onClick={() => handleToggle(s.server_id, !s.enabled)}
                    aria-pressed={s.enabled}
                    aria-label={
                      s.enabled ? `停用 ${s.server_id}` : `启用 ${s.server_id}`
                    }
                  >
                    {s.enabled ? "停用" : "启用"}
                  </button>
                  <button
                    type="button"
                    className="btn btn-sm btn-danger"
                    onClick={() => handleRemove(s.server_id)}
                    aria-label={`移除 ${s.server_id}`}
                  >
                    移除
                  </button>
                </td>
              </tr>
            ))}
            {servers.length === 0 && (
              <tr>
                <td colSpan={10} className="empty">
                  暂无已注册 MCP Server
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
