import { Fragment, useEffect, useState, useCallback } from "react";
import { getDagPlan, listDagHistory } from "../api";
import type { DagPlanDetail, DagPlanSummary, DagStatusFilter } from "../types";

const PAGE_SIZE = 20;

const STATUS_FILTERS: { value: DagStatusFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "running", label: "运行中" },
  { value: "succeeded", label: "已完成" },
  { value: "failed", label: "失败" },
  { value: "cancelled", label: "已取消" },
];

/** 状态 → 徽章类与中文标签。 */
function statusMeta(status: string): { cls: string; label: string } {
  switch (status) {
    case "pending":
      return { cls: "pill", label: "待执行" };
    case "running":
      return { cls: "pill pill-run", label: "运行中" };
    case "succeeded":
      return { cls: "pill pill-on", label: "已完成" };
    case "failed":
      return { cls: "pill pill-fail", label: "失败" };
    case "partially_succeeded":
      return { cls: "pill pill-warn", label: "部分成功" };
    case "cancelled":
      return { cls: "pill pill-off", label: "已取消" };
    default:
      return { cls: "pill", label: status };
  }
}

function StatusPill({ status }: { status: string }) {
  const { cls, label } = statusMeta(status);
  return <span className={cls}>{label}</span>;
}

function DagEmptyState({
  filter,
  onResetFilter,
}: {
  filter: DagStatusFilter;
  onResetFilter: () => void;
}): JSX.Element {
  const handleTry = (): void => {
    // B6:单一路径——App 侧 voicepilot:navigate 事件负责 setView("main")
    window.dispatchEvent(
      new CustomEvent("voicepilot:navigate", { detail: "main" }),
    );
  };
  const filtered = filter !== "all";
  const filterLabel =
    STATUS_FILTERS.find((f) => f.value === filter)?.label ?? "";
  return (
    <div
      className="card shell dag-empty-shell"
      role="region"
      aria-label="DAG 历史空状态"
    >
      <div className="card-inner dag-empty">
        <div className="dag-empty-illustration" aria-hidden="true">
          <svg
            viewBox="0 0 320 180"
            width="320"
            height="180"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
          >
            <rect
              x="0.5"
              y="0.5"
              width="319"
              height="179"
              rx="14"
              stroke="rgba(34,211,238,0.12)"
              strokeDasharray="6 6"
            />
            <circle
              cx="72"
              cy="90"
              r="28"
              stroke="rgba(34,211,238,0.9)"
              strokeWidth="1.4"
              fill="rgba(34,211,238,0.08)"
            />
            <circle
              cx="160"
              cy="48"
              r="24"
              stroke="rgba(34,211,238,0.7)"
              strokeWidth="1.2"
              fill="rgba(34,211,238,0.06)"
            />
            <circle
              cx="160"
              cy="132"
              r="24"
              stroke="rgba(34,211,238,0.7)"
              strokeWidth="1.2"
              fill="rgba(34,211,238,0.06)"
            />
            <circle
              cx="248"
              cy="90"
              r="28"
              stroke="rgba(34,211,238,0.9)"
              strokeWidth="1.4"
              fill="rgba(34,211,238,0.1)"
            />
            <path
              d="M98 82 L136 60"
              stroke="rgba(34,211,238,0.5)"
              strokeWidth="1.2"
              strokeLinecap="round"
              markerEnd="url(#arrow)"
            />
            <path
              d="M98 98 L136 122"
              stroke="rgba(34,211,238,0.5)"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
            <path
              d="M184 60 L224 82"
              stroke="rgba(34,211,238,0.5)"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
            <path
              d="M184 120 L224 98"
              stroke="rgba(34,211,238,0.5)"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
            <defs>
              <marker
                id="arrow"
                viewBox="0 0 10 10"
                refX="8"
                refY="5"
                markerWidth="6"
                markerHeight="6"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 10 5 L 0 10 z" fill="rgba(34,211,238,0.4)" />
              </marker>
            </defs>
            <text
              x="72"
              y="94"
              textAnchor="middle"
              fontSize="10"
              fontFamily="Cascadia Code, Consolas, monospace"
              fill="rgba(232,237,244,0.9)"
            >
              输入
            </text>
            <text
              x="160"
              y="52"
              textAnchor="middle"
              fontSize="9"
              fontFamily="Cascadia Code, Consolas, monospace"
              fill="rgba(232,237,244,0.85)"
            >
              审批
            </text>
            <text
              x="160"
              y="136"
              textAnchor="middle"
              fontSize="9"
              fontFamily="Cascadia Code, Consolas, monospace"
              fill="rgba(232,237,244,0.85)"
            >
              执行
            </text>
            <text
              x="248"
              y="94"
              textAnchor="middle"
              fontSize="10"
              fontFamily="Cascadia Code, Consolas, monospace"
              fill="#67e8f9"
            >
              结果
            </text>
          </svg>
        </div>
        {filtered ? (
          <h3 className="dag-empty-title">没有「{filterLabel}」的计划</h3>
        ) : (
          <h3 className="dag-empty-title">还没有 DAG 计划</h3>
        )}
        {filtered ? (
          <p className="dag-empty-desc">
            当前筛选「{filterLabel}」为空。试试其他状态,或清除过滤查看全部计划。
          </p>
        ) : (
          <p className="dag-empty-desc">
            DAG 是 LLM
            把你的一句话拆成的多步可审批任务。执行后在这里查看节点、状态与成功率。
          </p>
        )}
        {filtered ? (
          <button
            type="button"
            className="btn btn-primary btn-pill group dag-empty-cta"
            onClick={onResetFilter}
          >
            清除过滤
            <span className="btn-icon-circle" aria-hidden="true">
              ×
            </span>
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary btn-pill group dag-empty-cta"
            onClick={handleTry}
          >
            去 Main Chat 试一句
            <span className="btn-icon-circle" aria-hidden="true">
              →
            </span>
          </button>
        )}
        {!filtered && (
          <div className="dag-empty-ghosts" aria-label="示例指令">
            <button type="button" className="suggest-chip" onClick={handleTry}>
              整理下载目录的 PDF 到文档
            </button>
            <button type="button" className="suggest-chip" onClick={handleTry}>
              打开记事本写一条 TODO
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function DagHistoryView(): JSX.Element {
  const [plans, setPlans] = useState<DagPlanSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<DagStatusFilter>("all");
  const [offset, setOffset] = useState(0);
  // 3-8:翻页信号从显式布尔取,不再用 length===PAGE_SIZE 推断(恰好整页时会翻出空页)
  const [hasNext, setHasNext] = useState(false);
  const [expandedPlanId, setExpandedPlanId] = useState<string | null>(null);
  const [planDetail, setPlanDetail] = useState<DagPlanDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const refresh = useCallback((): void => {
    setLoading(true);
    setError(null);
    // 3-8:多取 1 条判断是否还有下一页,截断后展示,不动后端分页契约
    listDagHistory(PAGE_SIZE + 1, offset, filter)
      .then((result) => {
        const hasMore = result.length > PAGE_SIZE;
        setPlans(hasMore ? result.slice(0, PAGE_SIZE) : result);
        setHasNext(hasMore);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setHasNext(false);
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

  const hasPrevPage = offset > 0;

  return (
    <section aria-labelledby="dag-history-heading">
      <header className="view-head">
        <h1 id="dag-history-heading" className="view-title">
          <span className="view-kicker">DAG</span>
          DAG 历史
        </h1>
        <p className="view-desc">
          查看 LLM 拆解生成的 DAG 计划执行历史。点击行展开节点详情。
        </p>
      </header>

      {/* 状态过滤 */}
      <div className="dag-filter-bar" role="toolbar" aria-label="DAG 历史过滤">
        <div className="chips" role="radiogroup" aria-label="状态过滤">
          {STATUS_FILTERS.map((f) => (
            <button
              key={f.value}
              type="button"
              className={`chip ${filter === f.value ? "chip-active" : ""}`}
              onClick={() => handleFilterChange(f.value)}
              aria-pressed={filter === f.value}
              role="radio"
              aria-checked={filter === f.value}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>
      {/* 分页:B1 只在有数据时渲染,空状态下不再出现无意义分页条 */}
      {!loading && !error && plans.length > 0 && (
        <div
          className="dag-pagination-bar"
          role="toolbar"
          aria-label="DAG 历史分页"
        >
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
            disabled={!hasPrevPage}
            aria-label="上一页"
          >
            上一页
          </button>
          <span className="mono-sm" aria-live="polite">
            {`${offset + 1}–${offset + plans.length} 条${hasNext ? " · 还有更多" : ""}`}
          </span>
          <button
            type="button"
            className="btn btn-sm"
            onClick={() => setOffset(offset + PAGE_SIZE)}
            disabled={!hasNext}
            aria-label="下一页"
          >
            下一页
          </button>
        </div>
      )}

      {loading && (
        <div role="status" aria-live="polite">
          加载 DAG 历史…
        </div>
      )}
      {error && (
        <div className="alert alert-error" role="alert">
          <span className="alert-icon">⨯</span>
          <span>{error}</span>
        </div>
      )}

      {!loading && !error && plans.length === 0 && (
        <DagEmptyState
          filter={filter}
          onResetFilter={() => handleFilterChange("all")}
        />
      )}
      {!loading && !error && plans.length > 0 && (
        <div className="table-wrap dag-table-wrap card shell">
          <div className="card-inner">
            <table className="data-table" aria-label="DAG 历史列表">
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
                        className={`row-click ${isExpanded ? "row-expanded" : ""}`}
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
                        <td className="mono">{plan.plan_id.slice(0, 8)}…</td>
                        <td className="cell-goal">{plan.user_goal}</td>
                        <td>
                          <StatusPill status={plan.status} />
                        </td>
                        <td className="mono">{plan.created_at}</td>
                        <td className="mono">{plan.completed_at ?? "—"}</td>
                        <td className="mono">{plan.node_count}</td>
                        <td className="mono">
                          {(plan.success_rate * 100).toFixed(0)}%
                        </td>
                      </tr>
                      {isExpanded && (
                        <tr>
                          <td colSpan={7} className="cell-inset">
                            <div
                              id={`detail-${plan.plan_id}`}
                              className="dag-detail-panel"
                            >
                              {detailLoading && (
                                <div role="status">加载节点详情…</div>
                              )}
                              {detailError && (
                                <div className="alert alert-error" role="alert">
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
              </tbody>
            </table>
          </div>
        </div>
      )}
    </section>
  );
}

/** DAG 详情（展开行时显示节点列表）。 */
function DagPlanDetailAccordion({
  detail,
}: {
  detail: DagPlanDetail;
}): JSX.Element {
  return (
    <div>
      <div className="composer-meta tight">
        <span className="field-label">Plan ID：</span>
        <code className="mono">{detail.plan_id}</code>
        <span className="field-label">上限：</span>
        <span className="mono-sm">{detail.max_total_steps} 步</span>
      </div>
      <h4 className="section-title">节点（{detail.nodes.length}）</h4>
      <div className="node-grid">
        {detail.nodes.map((node) => (
          <div key={node.node_id} className="node-card">
            <div className="node-head">
              <span className="node-id">{node.node_id}</span>
              <span className="node-skill">{node.skill_id}</span>
              <span
                className={`risk-badge risk-${node.risk_ceiling.toLowerCase()}`}
              >
                {node.risk_ceiling}
              </span>
              <StatusPill status={node.status} />
            </div>
            {node.error_message && (
              <div className="alert alert-error alert-tight" role="alert">
                {node.error_message}
              </div>
            )}
            {node.output_json && (
              <details className="tpl-toggle">
                <summary>输出</summary>
                <pre className="tpl-pre">{node.output_json}</pre>
              </details>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
