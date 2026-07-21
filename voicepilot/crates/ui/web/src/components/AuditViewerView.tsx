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
    listAuditForTask(taskId)
      .then(setTaskEvents)
      .catch((e) => setError(String(e)));
  };

  const uniqueTaskIds = Array.from(new Set(recent.map((e) => e.task_id)));

  return (
    <section className="view-container audit-viewer" aria-labelledby="audit-heading">
      <h2 id="audit-heading">§ 8.3 Audit Viewer</h2>
      {loading && <div role="status" aria-live="polite">加载审计日志中…</div>}
      {error && <div className="form-error" role="alert">错误:{error}</div>}

      <div className="audit-layout">
        <div className="audit-task-list" role="list" aria-label="任务列表">
          <h3>最近任务</h3>
          {uniqueTaskIds.length === 0 && <p className="empty-state">暂无审计记录</p>}
          {uniqueTaskIds.map((taskId) => (
            <button
              key={taskId}
              type="button"
              role="listitem"
              className={`audit-task-item ${selectedTask === taskId ? "selected" : ""}`}
              onClick={() => handleSelectTask(taskId)}
              aria-pressed={selectedTask === taskId}
            >
              {taskId}
            </button>
          ))}
        </div>

        <div className="audit-timeline" aria-label="审计事件时间线">
          <h3>{selectedTask ? `任务 ${selectedTask} 的事件` : "选择一个任务查看详情"}</h3>
          {selectedTask && taskEvents.length === 0 && <p className="empty-state">该任务暂无事件</p>}
          <ol className="timeline-list">
            {taskEvents.map((event) => (
              <li key={event.log_id} className="timeline-item">
                <div className="timeline-time">{new Date(event.timestamp).toLocaleString()}</div>
                <div className="timeline-event">{event.event_type}</div>
                {event.step_id && <div className="timeline-step">step: {event.step_id}</div>}
                <div className="timeline-hash">hash: {event.hash.substring(0, 12)}…</div>
                {!event.prev_hash && <span className="timeline-genesis" aria-label="创世事件">⚡</span>}
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
