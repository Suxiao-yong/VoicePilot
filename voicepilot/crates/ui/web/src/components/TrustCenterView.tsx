import { useEffect, useState } from "react";
import { listMcpServers, toggleMcpServer } from "../api";
import type { McpServer } from "../types";

export function TrustCenterView(): JSX.Element {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

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

  return (
    <section className="view-container trust-center" aria-labelledby="trust-heading">
      <h2 id="trust-heading">§ 8.3 Trust Center</h2>
      <p className="view-description">管理 MCP Server 启用状态。停用后该 Server 不会被 Action Gateway 调用。</p>
      {loading && <div role="status" aria-live="polite">加载 MCP Server 列表…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

      <table className="mcp-table" aria-label="MCP Server 列表">
        <thead>
          <tr>
            <th scope="col">Server ID</th>
            <th scope="col">名称</th>
            <th scope="col">版本</th>
            <th scope="col">传输</th>
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
              <td>{s.trusted ? "✓" : "—"}</td>
              <td className="mono">{s.protocol_version ?? "—"}</td>
              <td className="mono small">{s.allowed_paths ?? "—"}</td>
              <td>
                <span className={`status-pill ${s.enabled ? "enabled" : "disabled"}`}>
                  {s.enabled ? "已启用" : "已停用"}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className={`toggle-btn ${s.enabled ? "disable" : "enable"}`}
                  onClick={() => handleToggle(s.server_id, !s.enabled)}
                  aria-pressed={s.enabled}
                  aria-label={s.enabled ? `停用 ${s.server_id}` : `启用 ${s.server_id}`}
                >
                  {s.enabled ? "停用" : "启用"}
                </button>
              </td>
            </tr>
          ))}
          {servers.length === 0 && (
            <tr>
              <td colSpan={9} className="empty-state">暂无已注册 MCP Server</td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}
