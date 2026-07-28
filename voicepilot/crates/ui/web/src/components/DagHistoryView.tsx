import { Fragment, useEffect, useState, useCallback } from "react";
import { getDagPlan, listDagHistory } from "../api";
import type {
  DagPlanDetail,
  DagPlanSummary,
  DagStatusFilter,
} from "../types";

const PAGE_SIZE = 20;

const STATUS_FILTERS: { value: DagStatusFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "running", label: "运行中" },
  { value: "succeeded", label: "已完成" },
  { value: "failed", label: "失败" },
  { value: "cancelled", label: "已取消" },
];

/** 状态徽章颜色编码(参考 UI 设计要求)。 */
function statusBadgeClass(status: string): string {
  switch (status) {
    case "pending":
      return "status-pill status-pending";
    case "running":
      return "status-pill status-running";
    case "succeeded":
      return "status-pill status-enabled";
    case "failed":
    case "partially_succeeded":
      return "status-pill status-failed";
    case "cancelled":
      return "status-pill status-disabled";
    default:
      return "status-pill";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "pending":
      return "待执行";
    case "running":
      return "运行中";
    case "succeeded":
      return "已完成";
    case "failed":
      return "失败";
    case "partially_succeeded":
      return "部分成功";
    case "cancelled":
      return "已取消";
    default:
      return status;
  }
}

export function DagHistoryView(): JSX.Element {
  const [plans, setPlans] = useState<DagPlanSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<DagStatusFilter>("all");
  const [offset, setOffset] = useState(0);
  const [expandedPlanId, setExpandedPlanId] = useState<string | null>(null);
  const [planDetail, setPlanDetail] = useState<DagPlanDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const refresh = useCallback((): void => {
    setLoading(true);
    setError(null);
    listDagHistory(PAGE_SIZE, offset, filter)
      .then((result) => {
        setPlans(result);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [offset, filter]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleFilterChange = (next: DagStatusFilter): void => {
    setFilter(next);
    setOffset(0);
    setExpandedPlanId(null);
    setPlanDetail(null);
  };

  const handleRowClick = (planId: string): void => {
    if (expandedPlanId === planId) {
      // 收起
      setExpandedPlanId(null);
      setPlanDetail(null);
      return;
    }
    setExpandedPlanId(planId);
    setPlanDetail(null);
    setDetailError(null);
    setDetailLoading(true);
    getDagPlan(planId)
      .then((detail) => {
        setPlanDetail(detail);
        setDetailLoading(false);
      })
      .catch((e) => {
        setDetailError(String(e));
        setDetailLoading(false);
      });
  };

  const hasNextPage = plans.length === PAGE_SIZE;
  const hasPrevPage = offset > 0;

  return (
    <section
      className="view-container dag-history"
      aria-labelledby="dag-history-heading"
    >
      <h2 id="dag-history-heading">DAG 历史</h2>
      <p className="view-description">
        查看 LLM 拆解生成的 DAG 计划执行历史。点击行展开节点详情。
      </p>

      {/* 状态过滤 + 分页 */}
      <div
        className="dag-history-controls"
        role="toolbar"
        aria-label="DAG 历史过滤与分页"
      >
        <div
          className="dag-filter-group"
          role="radiogroup"
          aria-label="状态过滤"
        >
          {STATUS_FILTERS.map((f) => (
            <button
              key={f.value}
              type="button"
              className={`filter-btn ${filter === f.value ? "active" : ""}`}
              onClick={() => handleFilterChange(f.value)}
              aria-pressed={filter === f.value}
              role="radio"
              aria-checked={filter === f.value}
            >
              {f.label}
            </button>
          ))}
        </div>
        <div className="dag-pagination">
          <button
            type="button"
            className="btn"
            onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
            disabled={!hasPrevPage}
            aria-label="上一页"
          >
            上一页
          </button>
          <span className="dag-page-info mono" aria-live="polite">
            {offset + 1}-{offset + plans.length}
          </span>
          <button
            type="button"
            className="btn"
            onClick={() => setOffset(offset + PAGE_SIZE)}
            disabled={!hasNextPage}
            aria-label="下一页"
          >
            下一页
          </button>
        </div>
      </div>

      {loading && (
        <div role="status" aria-live="polite">
          加载 DAG 历史…
        </div>
      )}
      {error && (
        <div className="form-error" role="alert">
          错误:{error}
        </div>
      )}

      {/* 表格 */}
      {!loading && !error && (
        <table className="dag-history-table" aria-label="DAG 历史列表">
          <thead>
            <tr>
              <th scope="col">Plan ID</th>
              <th scope="col">用户意图</th>
              <th scope="col">状态</th>
              <th scope="col">创建时间</th>
              <th scope="col">完成时间</th>
              <th scope="col">节点数</th>
              <th scope="col">成功率</th>
            </tr>
          </thead>
          <tbody>
            {plans.map((plan) => {
              const isExpanded = expandedPlanId === plan.plan_id;
              return (
                <Fragment key={plan.plan_id}>
                  <tr
                    className={`dag-history-row ${isExpanded ? "expanded" : ""}`}
                    onClick={() => handleRowClick(plan.plan_id)}
                    aria-expanded={isExpanded}
                    aria-controls={`detail-${plan.plan_id}`}
                    role="button"
                    tabIndex={0}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        handleRowClick(plan.plan_id);
                      }
                    }}
                  >
                    <td className="mono small">{plan.plan_id.slice(0, 8)}…</td>
                    <td className="dag-goal-cell">{plan.user_goal}</td>
                    <td>
                      <span className={statusBadgeClass(plan.status)}>
                        {statusLabel(plan.status)}
                      </span>
                    </td>
                    <td className="mono small">{plan.created_at}</td>
                    <td className="mono small">{plan.completed_at ?? "—"}</td>
                    <td className="mono">{plan.node_count}</td>
                    <td className="mono">
                      {(plan.success_rate * 100).toFixed(0)}%
                    </td>
                  </tr>
                  {isExpanded && (
                    <tr className="dag-detail-row">
                      <td colSpan={7}>
                        <div
                          id={`detail-${plan.plan_id}`}
                          className="dag-detail-panel"
                        >
                          {detailLoading && (
                            <div role="status">加载节点详情…</div>
                          )}
                          {detailError && (
                            <div className="form-error" role="alert">
                              {detailError}
                            </div>
                          )}
                          {planDetail && (
                            <DagPlanDetailAccordion detail={planDetail} />
                          )}
                        </div>
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
            {plans.length === 0 && (
              <tr>
                <td colSpan={7} className="empty-state">
                  暂无 DAG 历史记录
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** DAG 详情 accordion(展开行时显示节点列表)。 */
function DagPlanDetailAccordion({
  detail,
}: {
  detail: DagPlanDetail;
}): JSX.Element {
  return (
    <div className="dag-detail-accordion">
      <div className="dag-detail-summary">
        <span className="dag-label">Plan ID:</span>
        <code className="mono">{detail.plan_id}</code>
        <span className="dag-label" style={{ marginLeft: 16 }}>
          上限:
        </span>
        <span className="mono">{detail.max_total_steps} 步</span>
      </div>
      <h4 className="dag-section-title">节点({detail.nodes.length})</h4>
      <div className="dag-nodes-list">
        {detail.nodes.map((node) => (
          <div key={node.node_id} className="dag-node-card">
            <div className="dag-node-header">
              <span className="dag-node-id mono">{node.node_id}</span>
              <span className="dag-node-skill mono">{node.skill_id}</span>
              <span
                className={`risk-badge risk-badge-${node.risk_ceiling.toLowerCase()}`}
              >
                {node.risk_ceiling}
              </span>
              <span className={`status-pill status-${node.status}`}>
                {statusLabel(node.status)}
              </span>
            </div>
            {node.error_message && (
              <div className="dag-node-error" role="alert">
                错误:{node.error_message}
              </div>
            )}
            {node.output_json && (
              <details className="dag-template-preview">
                <summary className="dag-summary">输出</summary>
                <pre className="dag-template-code mono">{node.output_json}</pre>
              </details>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
