import { useEffect, useState } from "react";
import { listAuditRecent, listAuditForTask } from "../api";
import type { AuditEvent } from "../types";

export function AuditViewerView(): JSX.Element {
  const [recent, setRecent] = useState<AuditEvent[]>([]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const [taskEvents, setTaskEvents] = useState<AuditEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

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
