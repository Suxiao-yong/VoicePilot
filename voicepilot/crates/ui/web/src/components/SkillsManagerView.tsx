import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

// ponytail: preview-safe — plugin-dialog open() crashes outside Tauri (http://localhost:4173)
async function safeOpen(
  opts: Parameters<typeof open>[0],
): Promise<string | null> {
  try {
    const r = await open(opts);
    if (r === null) return null;
    return typeof r === "string" ? r : null;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (
      msg.includes("__TAURI") ||
      msg.includes("invoke") ||
      msg.includes("reading")
    ) {
      const wrapped = new Error(
        "文件选择仅在 Tauri 桌面端可用，请将 .md 放入 %APPDATA%\\voicepilot\\skills\\ 后点击“重新扫描”",
      );
      // SAFETY: `cause` 是 Error 标准属性，此处仅附加原始异常用于排障，不改变类型形状。
      (wrapped as unknown as { cause: unknown }).cause = e;
      throw wrapped;
    }
    throw e;
  }
}
import {
  invokeImportSkill,
  invokeListUserSkills,
  invokeReloadSkills,
  importExternalSkill,
  scanExternalSkills,
  listSkills,
  toggleSkill,
} from "../api";
import type { ExternalSkill, Skill, UserSkill } from "../types";

export function SkillsManagerView(): JSX.Element {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [userSkills, setUserSkills] = useState<UserSkill[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importNotice, setImportNotice] = useState<string | null>(null);
  const [externalSkills, setExternalSkills] = useState<ExternalSkill[] | null>(
    null,
  );
  const [scanningExternal, setScanningExternal] = useState(false);

  const refresh = (): void => {
    setLoading(true);
    Promise.all([listSkills(), invokeListUserSkills()])
      .then(([s, u]) => {
        setSkills(s);
        setUserSkills(u);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  };

  useEffect(refresh, []);

  const handleToggle = (skillId: string, enabled: boolean): void => {
    toggleSkill(skillId, enabled)
      .then(refresh)
      .catch((e) => setError(String(e)));
  };

  const handleImport = async (): Promise<void> => {
    setImporting(true);
    setImportNotice(null);
    setError(null);
    try {
      const selected = await safeOpen({
        multiple: false,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (selected === null) {
        setImporting(false);
        return;
      }
      const sourcePath = selected;
      if (sourcePath === null) {
        setImporting(false);
        return;
      }
      const imported = await invokeImportSkill(sourcePath);
      setImportNotice(`已导入：${imported.skill_id}（${imported.title}）`);
      const [s, u] = await Promise.all([listSkills(), invokeListUserSkills()]);
      setSkills(s);
      setUserSkills(u);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  };

  const handleReload = async (): Promise<void> => {
    setError(null);
    setImportNotice(null);
    try {
      const u = await invokeReloadSkills();
      setUserSkills(u);
      setImportNotice(`已重载 ${u.length} 个用户自定义 Skill`);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleScanExternal = async (): Promise<void> => {
    setError(null);
    setImportNotice(null);
    setScanningExternal(true);
    try {
      setExternalSkills(await scanExternalSkills());
    } catch (e) {
      setError(String(e));
    } finally {
      setScanningExternal(false);
    }
  };

  const handleImportExternal = async (dir: string): Promise<void> => {
    setError(null);
    setImportNotice(null);
    try {
      const imported = await importExternalSkill(dir);
      setImportNotice(
        `已导入：${imported.skill_id}（默认关闭，去列表手动启用）`,
      );
      const [s, u] = await Promise.all([listSkills(), invokeListUserSkills()]);
      setSkills(s);
      setUserSkills(u);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section aria-labelledby="skills-heading">
      <header className="view-head">
        <h1 id="skills-heading" className="view-title">
          <span className="view-kicker">Skills</span>
          Skill 管理
        </h1>
        <p className="view-desc">
          已保存 Skill 列表 + 成功率 + 平均延迟 + 风险等级。支持从
          <code className="mono"> %APPDATA%\voicepilot\skills\</code>{" "}
          导入用户自定义 Skill。
        </p>
      </header>

      {loading && (
        <div role="status" aria-live="polite">
          加载 Skill 列表…
        </div>
      )}
      {error && (
        <div className="alert alert-error" role="alert">
          <span className="alert-icon">⨯</span>
          <span>{error}</span>
        </div>
      )}
      {importNotice && !error && (
        <div className="alert alert-success" role="status" aria-live="polite">
          <span className="alert-icon">✓</span>
          <span>{importNotice}</span>
        </div>
      )}

      <div className="table-wrap">
        <table className="data-table" aria-label="Skill 列表">
          <thead>
            <tr>
              <th scope="col">Skill ID</th>
              <th scope="col">版本</th>
              <th scope="col">风险</th>
              <th scope="col">依赖</th>
              <th scope="col">成功次数</th>
              <th scope="col">平均延迟（ms）</th>
              <th scope="col">状态</th>
              <th scope="col">操作</th>
            </tr>
          </thead>
          <tbody>
            {skills.map((s) => (
              <tr key={s.skill_id}>
                <td className="mono">{s.skill_id}</td>
                <td className="mono">{s.version}</td>
                <td>
                  <span
                    className={`risk-badge risk-${s.risk_label.toLowerCase()}`}
                  >
                    {s.risk_label}
                  </span>
                </td>
                <td>
                  {s.skill_id === "research.save_markdown" ||
                  s.skill_id === "form.prepare" ? (
                    <span className="badge">Playwright MCP</span>
                  ) : (
                    <span className="mono cell-hash" aria-label="无依赖">
                      —
                    </span>
                  )}
                </td>
                <td className="mono">{s.success_count}</td>
                <td className="mono">{s.avg_latency_ms.toFixed(1)}</td>
                <td>
                  <span
                    className={`pill ${s.enabled ? "pill-on" : "pill-off"}`}
                  >
                    {s.enabled ? "已启用" : "已禁用"}
                  </span>
                </td>
                <td>
                  <button
                    type="button"
                    className={`btn btn-sm ${s.enabled ? "" : "btn-primary"}`}
                    onClick={() => handleToggle(s.skill_id, !s.enabled)}
                    aria-pressed={s.enabled}
                    aria-label={
                      s.enabled ? `禁用 ${s.skill_id}` : `启用 ${s.skill_id}`
                    }
                  >
                    {s.enabled ? "禁用" : "启用"}
                  </button>
                </td>
              </tr>
            ))}
            {skills.length === 0 && (
              <tr>
                <td colSpan={8} className="empty">
                  暂无已保存 Skill（执行 Skill 后自动记录）
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* ===== 用户自定义 Skill ===== */}
      <div className="card card-gap">
        <div className="card-head">
          <div>
            <h2 className="card-title">
              <span className="tick" aria-hidden="true" />
              用户自定义 Skill
            </h2>
            <p className="card-desc">
              把标准 SKILL.md 目录放入
              <code className="mono"> %APPDATA%\voicepilot\skills\</code>（
              <code className="mono">…\skills\&lt;name&gt;\SKILL.md</code>，与
              Claude Code / Codex 格式一致）或点击「导入 Skill」选择
              <code className="mono"> SKILL.md</code> 文件。同 id 用户 Skill
              自动覆盖 built-in。
            </p>
          </div>
          <div className="composer-meta tight">
            <button
              type="button"
              className="btn btn-primary"
              onClick={handleImport}
              disabled={importing}
              aria-busy={importing}
              aria-label="从本地选择 SKILL.md 文件导入为用户自定义 Skill"
            >
              {importing ? "导入中…" : "导入 Skill"}
            </button>
            <button
              type="button"
              className="btn"
              onClick={handleReload}
              aria-label="重新扫描 skills 目录"
            >
              重新扫描
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => void handleScanExternal()}
              disabled={scanningExternal}
              aria-busy={scanningExternal}
              aria-label="扫描全局第三方 Skill 目录（Claude Code / Agent Skills 标准位置，只读）"
            >
              {scanningExternal ? "扫描中…" : "扫描外部 Skills"}
            </button>
          </div>
        </div>

        <div className="table-wrap">
          <table className="data-table" aria-label="用户自定义 Skill 列表">
            <thead>
              <tr>
                <th scope="col">Skill ID</th>
                <th scope="col">标题</th>
                <th scope="col">描述</th>
                <th scope="col">类型</th>
                <th scope="col">源文件路径</th>
              </tr>
            </thead>
            <tbody>
              {userSkills.map((u) => (
                <tr key={u.skill_id}>
                  <td className="mono">{u.skill_id}</td>
                  <td>{u.title}</td>
                  <td>{u.description}</td>
                  <td>
                    {u.executable ? (
                      <span className="pill pill-on">可执行</span>
                    ) : (
                      <span className="pill">展示</span>
                    )}
                  </td>
                  <td className="mono cell-hash">{u.source_path}</td>
                </tr>
              ))}
              {userSkills.length === 0 && (
                <tr>
                  <td colSpan={5} className="empty">
                    暂无用户自定义 Skill
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* ===== 外部发现：全局第三方 Skills（只读扫描，导入默认关闭） ===== */}
      {externalSkills !== null && (
        <div className="card card-gap">
          <div className="card-head">
            <div>
              <h2 className="card-title">
                <span className="tick" aria-hidden="true" />
                外部发现 Skills
              </h2>
              <p className="card-desc">
                来自 Claude Code / Agent Skills
                标准位置。导入后默认关闭，去上表手动启用。
              </p>
            </div>
          </div>
          <div className="table-wrap">
            <table className="data-table" aria-label="外部发现 Skill 列表">
              <thead>
                <tr>
                  <th scope="col">Skill ID</th>
                  <th scope="col">标题</th>
                  <th scope="col">描述</th>
                  <th scope="col">类型</th>
                  <th scope="col">来源</th>
                  <th scope="col">目录</th>
                  <th scope="col">状态</th>
                  <th scope="col">操作</th>
                </tr>
              </thead>
              <tbody>
                {externalSkills.map((hit) => {
                  const installed = userSkills.some(
                    (u) => u.skill_id === hit.id,
                  );
                  return (
                    <tr key={`${hit.source}:${hit.id}`}>
                      <td className="mono">{hit.id}</td>
                      <td>{hit.title}</td>
                      <td>{hit.description}</td>
                      <td>
                        {hit.executable ? (
                          <span
                            className="pill pill-on"
                            title={
                              hit.exec_server
                                ? `调用 ${hit.exec_server} / ${hit.exec_tool ?? ""}，启用后生效`
                                : "绑定可执行工具，启用后生效"
                            }
                          >
                            可执行
                          </span>
                        ) : (
                          <span className="pill">展示</span>
                        )}
                      </td>
                      <td>
                        {/* Phase A 去重：同 id 同内容多来源合并为一个候选，
                            来源列表全部展示。 */}
                        <span className="pill">{hit.sources.join("\n")}</span>
                      </td>
                      <td className="mono cell-hash">{hit.dir}</td>
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
                          onClick={() => void handleImportExternal(hit.dir)}
                          aria-label={`导入外部 Skill ${hit.id}`}
                        >
                          导入
                        </button>
                      </td>
                    </tr>
                  );
                })}
                {externalSkills.length === 0 && (
                  <tr>
                    <td colSpan={8} className="empty">
                      未发现外部 Skills（Claude Code / .agents
                      目录为空或不存在）
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </section>
  );
}
