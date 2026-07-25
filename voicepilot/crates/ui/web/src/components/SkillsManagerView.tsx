import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  invokeImportSkill,
  invokeListUserSkills,
  invokeReloadSkills,
  listSkills,
  toggleSkill,
} from "../api";
import type { Skill, UserSkill } from "../types";

export function SkillsManagerView(): JSX.Element {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [userSkills, setUserSkills] = useState<UserSkill[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importNotice, setImportNotice] = useState<string | null>(null);

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
    toggleSkill(skillId, enabled).then(refresh).catch((e) => setError(String(e)));
  };

  const handleImport = async (): Promise<void> => {
    setImporting(true);
    setImportNotice(null);
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (selected === null) {
        // 用户取消选择 — 不是错误,静默退出。
        setImporting(false);
        return;
      }
      // open() 单选返回 string | null(单选时不返回数组)。
      const sourcePath = typeof selected === "string" ? selected : null;
      if (sourcePath === null) {
        setImporting(false);
        return;
      }
      const imported = await invokeImportSkill(sourcePath);
      setImportNotice(`已导入:${imported.skill_id}(${imported.title})`);
      // 刷新用户 + DB 两侧列表。
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

  return (
    <section
      className="view-container skills-manager"
      aria-labelledby="skills-heading"
    >
      <h2 id="skills-heading">§ 8.3 Skills Manager</h2>
      <p className="view-description">
        已保存 Skill 列表 + 成功率 + 平均延迟 + 风险等级。W7 Plan 3 起支持从
        <code className="skills-dir-hint">%APPDATA%\voicepilot\skills\</code>
        导入用户自定义 Skill(.md)。
      </p>
      {loading && (
        <div role="status" aria-live="polite">
          加载 Skill 列表…
        </div>
      )}
      {error && (
        <div className="form-error" role="alert">
          错误:{error}
        </div>
      )}
      {importNotice && !error && (
        <div className="import-notice" role="status" aria-live="polite">
          {importNotice}
        </div>
      )}

      <table className="skill-table" aria-label="Skill 列表">
        <thead>
          <tr>
            <th scope="col">Skill ID</th>
            <th scope="col">版本</th>
            <th scope="col">风险</th>
            <th scope="col">成功次数</th>
            <th scope="col">平均延迟(ms)</th>
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
                <span className={`risk-pill risk-${s.risk_label.toLowerCase()}`}>
                  {s.risk_label}
                </span>
              </td>
              <td className="mono">{s.success_count}</td>
              <td className="mono">{s.avg_latency_ms.toFixed(1)}</td>
              <td>
                <span className={`status-pill ${s.enabled ? "enabled" : "disabled"}`}>
                  {s.enabled ? "已启用" : "已禁用"}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className={`toggle-btn ${s.enabled ? "disable" : "enable"}`}
                  onClick={() => handleToggle(s.skill_id, !s.enabled)}
                  aria-pressed={s.enabled}
                  aria-label={s.enabled ? `禁用 ${s.skill_id}` : `启用 ${s.skill_id}`}
                >
                  {s.enabled ? "禁用" : "启用"}
                </button>
              </td>
            </tr>
          ))}
          {skills.length === 0 && (
            <tr>
              <td colSpan={7} className="empty-state">
                暂无已保存 Skill(执行 Skill 后自动记录)
              </td>
            </tr>
          )}
        </tbody>
      </table>

      {/* ===== W7 Plan 3: 用户自定义 Skill 区 ===== */}
      <div className="user-skills-section" aria-labelledby="user-skills-heading">
        <div className="user-skills-header">
          <h3 id="user-skills-heading">用户自定义 Skill</h3>
          <div className="user-skills-actions">
            <button
              type="button"
              className="btn btn-primary user-skill-btn"
              onClick={handleImport}
              disabled={importing}
              aria-busy={importing}
              aria-label="从本地选择 .md 文件导入为用户自定义 Skill"
            >
              {importing ? "导入中…" : "导入 Skill"}
            </button>
            <button
              type="button"
              className="btn user-skill-btn"
              onClick={handleReload}
              aria-label="重新扫描 skills 目录"
            >
              重新扫描
            </button>
          </div>
        </div>
        <p className="view-description">
          把 .md 文件放入
          <code className="skills-dir-hint">%APPDATA%\voicepilot\skills\</code>
          或点击「导入 Skill」选择本地文件。同 id 用户 Skill 自动覆盖 built-in。
        </p>

        <table className="skill-table" aria-label="用户自定义 Skill 列表">
          <thead>
            <tr>
              <th scope="col">Skill ID</th>
              <th scope="col">标题</th>
              <th scope="col">描述</th>
              <th scope="col">源文件路径</th>
            </tr>
          </thead>
          <tbody>
            {userSkills.map((u) => (
              <tr key={u.skill_id}>
                <td className="mono">{u.skill_id}</td>
                <td>{u.title}</td>
                <td className="user-skill-desc">{u.description}</td>
                <td className="mono user-skill-path">{u.source_path}</td>
              </tr>
            ))}
            {userSkills.length === 0 && (
              <tr>
                <td colSpan={4} className="empty-state">
                  暂无用户自定义 Skill
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
