import { useEffect, useState } from "react";
import {
  listAuditRecent,
  listAuditForTask,
  listCompensableActions,
  getObservability,
} from "../api";
import type {
  AuditEvent,
  CompensableAction,
  ObservabilitySnapshot,
} from "../types";
import { UndoButton } from "./UndoButton";

/** 延迟分位展示：无样本显示 "—"，否则 "N ms"。 */
function fmtMs(value: number | undefined): string {
  return value === undefined ? "—" : `${value} ms`;
}

export function AuditViewerView(): JSX.Element {
  const [recent, setRecent] = useState<AuditEvent[]>([]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [taskEvents, setTaskEvents] = useState<AuditEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // Task 2：延迟 / 缓存可观测快照 + 可补偿动作列表。
  const [snapshot, setSnapshot] = useState<ObservabilitySnapshot | null>(null);
  const [compensable, setCompensable] = useState<CompensableAction[]>([]);

  useEffect(() => {
    setLoading(true);
    listAuditRecent(50)
      .then((events) => {
        setRecent(events);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, []);

  // 可观测面板与撤销列表：独立加载，失败不阻塞审计时间线。
  useEffect(() => {
    let cancelled = false;
    getObservability()
      .then((s) => {
        if (!cancelled) setSnapshot(s);
      })
      .catch(() => undefined);
    listCompensableActions()
      .then((actions) => {
        if (!cancelled) setCompensable(actions);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSelectTask = (taskId: string): void => {
    setSelectedTask(taskId);
    setError(null);
    // 3-7:加载前清空上一任务事件,失败时时间线不再残留旧任务数据
    setTaskEvents([]);
    listAuditForTask(taskId)
      .then(setTaskEvents)
      .catch((e) => setError(String(e)));
  };

  const uniqueTaskIds = Array.from(new Set(recent.map((e) => e.task_id)));

  const bucket = (kind: string): ObservabilitySnapshot["routes"][number] | undefined =>
    snapshot?.routes.find((r) => r.route_kind === kind);
  const fast = bucket("fastroute");
  const keyword = bucket("keyword");
  const llm = bucket("llm");
  const hitRate =
    snapshot === null
      ? "—"
      : `${(snapshot.cache.hit_rate * 100).toFixed(1)}%`;

  return (
    <section aria-labelledby="audit-heading">
      <header className="view-head">
        <h1 id="audit-heading" className="view-title">
          <span className="view-kicker">Audit</span>
          审计日志
        </h1>
        <p className="view-desc">
          防篡改哈希链审计记录。选择任务查看完整事件时间线。
        </p>
      </header>

      {/* Task 2：可观测指标卡（路由延迟分位 / 缓存命中率 / 审计链完整性） */}
      <div className="org-summary" aria-label="可观测指标">
        <div className="org-stat">
          <div className="k">快路由 P50 / P95</div>
          <div className="v">
            {fmtMs(fast?.p50_ms)} / {fmtMs(fast?.p95_ms)}
          </div>
        </div>
        <div className="org-stat">
          <div className="k">关键词 P50 / P95</div>
          <div className="v">
            {fmtMs(keyword?.p50_ms)} / {fmtMs(keyword?.p95_ms)}
          </div>
        </div>
        <div className="org-stat">
          <div className="k">LLM P50 / P95</div>
          <div className="v">
            {fmtMs(llm?.p50_ms)} / {fmtMs(llm?.p95_ms)}
          </div>
        </div>
        <div className="org-stat">
          <div className="k">缓存命中率 / 审计链</div>
          <div className="v">{hitRate}</div>
          <div className="mono-sm">
            审计链：
            {snapshot === null
              ? "—"
              : snapshot.audit_chain.valid
                ? `完整（${snapshot.audit_chain.checked} 条）`
                : "已损坏"}
          </div>
        </div>
      </div>

      {/* Task 2：可补偿动作列表（一键撤销入口） */}
      <div className="card card-gap">
        <div className="card-head">
          <div>
            <h2 className="card-title">
              <span className="tick" aria-hidden="true" />
              可补偿动作
            </h2>
            <p className="card-desc">
              仍在 TTL 内的副作用可一键回滚；回滚后补偿记录标记为 reversed
            </p>
          </div>
        </div>
        {compensable.length === 0 && (
          <p className="empty">暂无可撤销动作</p>
        )}
        <ul className="tl-list" aria-label="可补偿动作列表">
          {compensable.map((action) => (
            <li key={action.comp_id} className="tl-item">
              <div className="tl-head">
                <span className="tl-event">{action.description}</span>
                <span className="pill">{action.level}</span>
              </div>
              <div className="tl-meta">
                <span>step: {action.step_id}</span>
                <span>ttl: {action.ttl_expires}</span>
                <UndoButton action={action} />
              </div>
            </li>
          ))}
        </ul>
      </div>

      {loading && (
        <div role="status" aria-live="polite">
          加载审计日志中…
        </div>
      )}
      {error && (
        <div className="alert alert-error" role="alert">
          <span className="alert-icon">⨯</span>
          <span>{error}</span>
        </div>
      )}

      <div className="audit-layout">
        <div className="audit-tasks" role="list" aria-label="任务列表">
          <div className="audit-tasks-title">最近任务</div>
          {uniqueTaskIds.length === 0 && <p className="empty">暂无审计记录</p>}
          {uniqueTaskIds.map((taskId) => (
            <button
              key={taskId}
              type="button"
              role="listitem"
              className={`task-item ${selectedTask === taskId ? "selected" : ""}`}
              onClick={() => handleSelectTask(taskId)}
              aria-pressed={selectedTask === taskId}
            >
              {taskId}
            </button>
          ))}
        </div>

        <div className="timeline" aria-label="审计事件时间线">
          <div className="card-title tl-heading">
            <span className="tick" aria-hidden="true" />
            {selectedTask
              ? `任务 ${selectedTask} 的事件`
              : "选择一个任务查看详情"}
          </div>
          {selectedTask && taskEvents.length === 0 && (
            <p className="empty">该任务暂无事件</p>
          )}
          <ol className="tl-list">
            {taskEvents.map((event) => (
              <li
                key={event.log_id}
                className={`tl-item ${!event.prev_hash ? "genesis" : ""}`}
              >
                <div className="tl-head">
                  <span className="tl-event">{event.event_type}</span>
                  <span className="tl-time">
                    {new Date(event.timestamp).toLocaleString()}
                  </span>
                  {!event.prev_hash && (
                    <span className="badge" aria-label="创世事件">
                      创世
                    </span>
                  )}
                </div>
                <div className="tl-meta">
                  {event.step_id && <span>step: {event.step_id}</span>}
                  <span>hash: {event.hash.substring(0, 12)}…</span>
                  {event.prev_hash && (
                    <span>prev: {event.prev_hash.substring(0, 12)}…</span>
                  )}
                </div>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
