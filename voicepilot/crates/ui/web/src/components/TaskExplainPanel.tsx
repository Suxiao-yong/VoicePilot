import { useEffect, useState } from "react";
import { getTaskExplanation } from "../api";
import type { TaskExplanation } from "../types";

interface Props {
  stepId: string;
  /** Step 状态(用于决定是否显示 LLM 归因段落;Failed 才有归因)。 */
  stepStatus?: string;
}

/** LLM 归因 category 徽章颜色。 */
function categoryBadgeClass(category: string): string {
  switch (category) {
    case "mcp_unavailable":
      return "category-badge category-mcp";
    case "path_not_allowed":
      return "category-badge category-path";
    case "approval_denied":
      return "category-badge category-approval";
    case "network_error":
      return "category-badge category-network";
    case "unknown":
      return "category-badge category-unknown";
    default:
      return "category-badge category-unknown";
  }
}

function categoryLabel(category: string): string {
  switch (category) {
    case "mcp_unavailable":
      return "MCP 不可用";
    case "path_not_allowed":
      return "路径不允许";
    case "approval_denied":
      return "审批被拒";
    case "network_error":
      return "网络错误";
    case "unknown":
      return "未知";
    default:
      return category;
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
    <section
      className="task-explain-panel"
      aria-labelledby="task-explain-heading"
    >
      <h3 id="task-explain-heading" className="task-explain-title">
        Task Explain · 失败归因
      </h3>

      {loading && (
        <div role="status" aria-live="polite">
          加载 LLM 归因…
        </div>
      )}
      {error && (
        <div className="form-error" role="alert">
          错误:{error}
        </div>
      )}

      {!loading && !error && (
        <>
          {/* Section 1: Step 基本信息 */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("info")}
              aria-expanded={openSection === "info"}
              aria-controls="explain-info"
            >
              <span className="explain-accordion-title">Step 基本信息</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "info" ? "−" : "+"}
              </span>
            </button>
            {openSection === "info" && (
              <div id="explain-info" className="explain-accordion-body">
                <div className="explain-info-row">
                  <span className="dag-label">Step ID</span>
                  <code className="mono">{stepId}</code>
                </div>
                {stepStatus && (
                  <div className="explain-info-row">
                    <span className="dag-label">状态</span>
                    <span className={`status-pill status-${stepStatus}`}>
                      {stepStatus}
                    </span>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Section 2: 失败工具调用列表(W8 简化:仅显示提示,完整列表需后端额外命令) */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("tools")}
              aria-expanded={openSection === "tools"}
              aria-controls="explain-tools"
            >
              <span className="explain-accordion-title">失败工具调用</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "tools" ? "−" : "+"}
              </span>
            </button>
            {openSection === "tools" && (
              <div id="explain-tools" className="explain-accordion-body">
                <p className="explain-empty-hint">
                  失败工具调用详情需查看 Audit Viewer 中该 step 的 audit_logs。
                </p>
              </div>
            )}
          </div>

          {/* Section 3: LLM 归因 */}
          <div className="explain-accordion">
            <button
              type="button"
              className="explain-accordion-header"
              onClick={() => toggleSection("llm")}
              aria-expanded={openSection === "llm"}
              aria-controls="explain-llm"
            >
              <span className="explain-accordion-title">LLM 归因</span>
              <span className="explain-accordion-icon" aria-hidden="true">
                {openSection === "llm" ? "−" : "+"}
              </span>
            </button>
            {openSection === "llm" && (
              <div id="explain-llm" className="explain-accordion-body">
                {explanation ? (
                  <div className="llm-analysis">
                    <div className="llm-row">
                      <span className="dag-label">根本原因</span>
                      <p className="llm-root-cause">{explanation.root_cause_zh}</p>
                    </div>
                    <div className="llm-row">
                      <span className="dag-label">分类</span>
                      <span
                        className={categoryBadgeClass(explanation.category)}
                        aria-label={`失败分类 ${categoryLabel(explanation.category)}`}
                      >
                        {categoryLabel(explanation.category)}
                      </span>
                    </div>
                    {explanation.suggested_fix && (
                      <div className="llm-row">
                        <span className="dag-label">建议修复</span>
                        <p className="llm-suggested-fix">
                          {explanation.suggested_fix}
                        </p>
                      </div>
                    )}
                    <div className="llm-row">
                      <span className="dag-label">置信度</span>
                      <div
                        className="confidence-bar"
                        role="meter"
                        aria-valuenow={Math.round(explanation.confidence * 100)}
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-label="LLM 归因置信度"
                      >
                        <div
                          className="confidence-fill"
                          style={{ width: `${explanation.confidence * 100}%` }}
                        />
                        <span className="confidence-value mono">
                          {(explanation.confidence * 100).toFixed(0)}%
                        </span>
                      </div>
                    </div>
                    {explanation.llm_model && (
                      <div className="llm-row">
                        <span className="dag-label">模型</span>
                        <code className="mono">{explanation.llm_model}</code>
                      </div>
                    )}
                  </div>
                ) : (
                  <p className="explain-empty-hint">
                    未启用 LLM 归因,可在 Settings 中开启(需配置 llm_api_key +
                    llm_enabled = true + privacy_mode = false)。
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
