import { useEffect, useState } from "react";
import { getTaskExplanation } from "../api";
import type { TaskExplanation } from "../types";

interface Props {
  stepId: string;
  /** Step 状态（用于决定是否显示 LLM 归因段落；Failed 才有归因）。 */
  stepStatus?: string;
}

/** LLM 归因 category 徽章。 */
function categoryMeta(category: string): { cls: string; label: string } {
  switch (category) {
    case "mcp_unavailable":
      return { cls: "pill pill-warn", label: "MCP 不可用" };
    case "path_not_allowed":
      return { cls: "pill pill-fail", label: "路径不允许" };
    case "approval_denied":
      return { cls: "pill pill-fail", label: "审批被拒" };
    case "network_error":
      return { cls: "pill pill-warn", label: "网络错误" };
    default:
      return { cls: "pill", label: category === "unknown" ? "未知" : category };
  }
}

export function TaskExplainPanel({ stepId, stepStatus }: Props): JSX.Element {
  const [explanation, setExplanation] = useState<TaskExplanation | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [openSection, setOpenSection] = useState<"info" | "tools" | "llm" | null>("llm");

  useEffect(() => {
    setLoading(true);
    setError(null);
    getTaskExplanation(stepId)
      .then((result) => {
        setExplanation(result);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [stepId]);

  const toggleSection = (section: "info" | "tools" | "llm"): void => {
    setOpenSection(openSection === section ? null : section);
  };

  return (
    <section className="explain-card" aria-labelledby="task-explain-heading">
      <div className="card-head explain-head">
        <h3 id="task-explain-heading" className="card-title">
          <span className="tick" aria-hidden="true" />
          Task Explain · 失败归因
        </h3>
      </div>

      {loading && (
        <div className="acc-pad" role="status" aria-live="polite">
          加载 LLM 归因…
        </div>
      )}
      {error && (
        <div className="alert alert-error alert-inset" role="alert">
          {error}
        </div>
      )}

      {!loading && !error && (
        <>
          <div className="acc-item">
            <button
              type="button"
              className="acc-head"
              onClick={() => toggleSection("info")}
              aria-expanded={openSection === "info"}
              aria-controls="explain-info"
            >
              <span>Step 基本信息</span>
              <span className="acc-icon" aria-hidden="true">
                {openSection === "info" ? "−" : "+"}
              </span>
            </button>
            {openSection === "info" && (
              <div id="explain-info" className="acc-body">
                <div className="llm-row">
                  <div className="k">Step ID</div>
                  <code className="mono">{stepId}</code>
                </div>
                {stepStatus && (
                  <div className="llm-row">
                    <div className="k">状态</div>
                    <span className="pill">{stepStatus}</span>
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="acc-item">
            <button
              type="button"
              className="acc-head"
              onClick={() => toggleSection("tools")}
              aria-expanded={openSection === "tools"}
              aria-controls="explain-tools"
            >
              <span>失败工具调用</span>
              <span className="acc-icon" aria-hidden="true">
                {openSection === "tools" ? "−" : "+"}
              </span>
            </button>
            {openSection === "tools" && (
              <div id="explain-tools" className="acc-body">
                <p className="diff-note">
                  失败工具调用详情需查看 Audit Viewer 中该 step 的 audit_logs。
                </p>
              </div>
            )}
          </div>

          <div className="acc-item">
            <button
              type="button"
              className="acc-head"
              onClick={() => toggleSection("llm")}
              aria-expanded={openSection === "llm"}
              aria-controls="explain-llm"
            >
              <span>LLM 归因</span>
              <span className="acc-icon" aria-hidden="true">
                {openSection === "llm" ? "−" : "+"}
              </span>
            </button>
            {openSection === "llm" && (
              <div id="explain-llm" className="acc-body">
                {explanation ? (
                  <div>
                    <div className="llm-row">
                      <div className="k">根本原因</div>
                      <p>{explanation.root_cause_zh}</p>
                    </div>
                    <div className="llm-row">
                      <div className="k">分类</div>
                      <span
                        className={categoryMeta(explanation.category).cls}
                        aria-label={`失败分类 ${categoryMeta(explanation.category).label}`}
                      >
                        {categoryMeta(explanation.category).label}
                      </span>
                    </div>
                    {explanation.suggested_fix && (
                      <div className="llm-row">
                        <div className="k">建议修复</div>
                        <p>{explanation.suggested_fix}</p>
                      </div>
                    )}
                    <div className="llm-row">
                      <div className="k">置信度</div>
                      <div
                        className="meter"
                        role="meter"
                        aria-valuenow={Math.round(explanation.confidence * 100)}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-label="LLM 归因置信度"
                      >
                        <div
                          className="meter-fill"
                          style={{ width: `${explanation.confidence * 100}%` }}
                        />
                      </div>
                      <span className="meter-value">
                        {(explanation.confidence * 100).toFixed(0)}%
                      </span>
                    </div>
                    {explanation.llm_model && (
                      <div className="llm-row">
                        <div className="k">模型</div>
                        <code className="mono">{explanation.llm_model}</code>
                      </div>
                    )}
                  </div>
                ) : (
                  <p className="diff-note">
                    未启用 LLM 归因，可在设置中开启（需配置 API Key + 启用 LLM + 关闭隐私模式）。
                  </p>
                )}
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
}
