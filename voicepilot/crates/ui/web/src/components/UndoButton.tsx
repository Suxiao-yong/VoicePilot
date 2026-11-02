import { useState } from "react";
import { newUndoInput, undoCompensation } from "../api";
import type { CompensableAction, UndoResult } from "../types";

/**
 * 一键撤销按钮（Task 2，契约 §2.4）。
 *
 * 点击 → `undoCompensation`（后端 `task.compensate` 全管道：auto_reverse →
 * verify → 补偿记录 status="reversed"）。成功后按钮原地替换为徽章
 * 「已补偿回滚」；失败显示后端错误文本；请求中禁用并显示「回滚中…」。
 */
export function UndoButton({
  action,
  onDone,
}: {
  action: CompensableAction;
  onDone?: (result: UndoResult) => void;
}): JSX.Element {
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onUndo(): Promise<void> {
    setBusy(true);
    setError(null);
    try {
      const result = await undoCompensation(newUndoInput(action));
      if (result.committed) {
        setDone(true);
      } else {
        setError(result.error ?? "回滚失败");
      }
      onDone?.(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  if (done) {
    return (
      <span className="badge" role="status" aria-live="polite">
        已补偿回滚
      </span>
    );
  }

  return (
    <span className="undo-wrap">
      <button
        type="button"
        className="btn btn-sm"
        onClick={onUndo}
        disabled={busy}
        title={action.description}
        aria-label={`撤销 ${action.step_id}`}
      >
        {busy ? "回滚中…" : "一键撤销"}
      </button>
      {error && (
        <span className="mono-sm" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}
