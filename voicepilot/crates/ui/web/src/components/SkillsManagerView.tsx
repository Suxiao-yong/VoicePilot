import { useEffect, useState } from "react";
import { listSkills, toggleSkill } from "../api";
import type { Skill } from "../types";

export function SkillsManagerView(): JSX.Element {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = (): void => {
    setLoading(true);
    listSkills()
      .then((s) => {
        setSkills(s);
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

  return (
    <section className="view-container skills-manager" aria-labelledby="skills-heading">
      <h2 id="skills-heading">§ 8.3 Skills Manager</h2>
      <p className="view-description">已保存 Skill 列表 + 成功率 + 平均延迟 + 风险等级。</p>
      {loading && <div role="status" aria-live="polite">加载 Skill 列表…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

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
              <td><span className={`risk-pill risk-${s.risk_label.toLowerCase()}`}>{s.risk_label}</span></td>
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
            <tr><td colSpan={7} className="empty-state">暂无已保存 Skill(执行 Skill 后自动记录)</td></tr>
          )}
        </tbody>
      </table>
    </section>
  );
}
