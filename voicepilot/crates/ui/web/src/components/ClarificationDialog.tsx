import { useEffect, useRef, useState } from "react";
import { submitClarification } from "../api";
import type { ClarificationRequestPayload } from "../types";
import { useDialogA11y } from "../useDialogA11y";

interface Props {
  payload: ClarificationRequestPayload;
  onDismiss: () => void;
}

/**
 * 追问卡 —— 含糊请求时让用户三选一（与审批语义分离）。
 * 关闭（Esc / 卸载）视为选默认项，仅已提交点选时跳过。
 */
export function ClarificationDialog({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submittedRef = useRef(false);
  const defaultBtnRef = useRef<HTMLButtonElement | null>(null);
  const { clarification_request_id, question, options, default_index } =
    payload;

  // 焦点圈禁 + Esc；初始焦点给默认项（追问不默认放行高危动作，只选下载源）。
  const dialogRef = useDialogA11y(onDismiss, { focusRef: defaultBtnRef });

  async function choose(index: number) {
    setSubmitting(true);
    setError(null);
    try {
      await submitClarification(clarification_request_id, index);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // 卸载时自动选默认项 —— submittedRef 短路：已提交则跳过。
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      submitClarification(clarification_request_id, default_index).catch(
        () => {},
      );
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="dialog-backdrop">
      <div
        ref={dialogRef}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="clarification-dialog-title"
        tabIndex={-1}
      >
        <div className="dialog-head">
          <h2 id="clarification-dialog-title" className="dialog-title">
            <span className="dot" aria-hidden="true" />
            请确认
          </h2>
          <span className="badge">追问 · 超时默认第一项</span>
        </div>
        <div className="dialog-body">
          <p className="clarify-question">{question}</p>
          <div className="clarify-options">
            {options.map((opt, i) => (
              <button
                key={i}
                ref={i === default_index ? defaultBtnRef : undefined}
                type="button"
                className={`btn clarify-option ${i === default_index ? "btn-primary" : ""}`}
                disabled={submitting}
                onClick={() => choose(i)}
              >
                {opt}
                {i === default_index ? "（默认）" : ""}
              </button>
            ))}
          </div>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
