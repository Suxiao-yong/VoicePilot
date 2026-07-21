import { useEffect, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { approval_request_id, manifest } = payload;

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    setError(null);
    try {
      await submitApproval(approval_request_id, decision);
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Esc 键关闭 modal —— 卸载时 cleanup effect 会自动发送 deny(一次性语义)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  // 卸载时自动拒绝(例如用户关闭窗口)
  useEffect(() => {
    return () => {
      // 关闭时尽力发送 deny —— 但仅当尚未提交
      // Rust 端如果已消费会返回 false(一次性)
      submitApproval(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="approval-modal-title">
        <div className="modal-header">
          <h2 id="approval-modal-title">Approve File Operation</h2>
          <span className="badge">E2 · D2 · Local</span>
        </div>
        <div className="modal-body">
          <div className="manifest-summary">
            <div className="summary-stat">
              <span className="label">Sources</span>
              <span className="value">{manifest.sources.length}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Total Bytes</span>
              <span className="value">{manifest.total_bytes.toLocaleString()}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Conflicts</span>
              <span className="value danger">
                {manifest.conflicts.length}
              </span>
            </div>
          </div>

          <table className="manifest-table">
            <thead>
              <tr>
                <th>Path</th>
                <th>Size</th>
                <th>SHA-256</th>
              </tr>
            </thead>
            <tbody>
              {manifest.sources.map((s) => (
                <tr key={s.canonical_path}>
                  <td className="path">{s.canonical_path}</td>
                  <td>{s.size}</td>
                  <td>{s.sha256.slice(0, 16)}…</td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="form-row" style={{ marginTop: 24 }}>
            <label htmlFor="approval-destination">Destination</label>
            <input id="approval-destination" type="text" value={manifest.destination} readOnly />
          </div>

          {manifest.conflicts.length > 0 && (
            <div className="conflicts-list">
              ⚠ {manifest.conflicts.length} conflict(s) detected:
              <ul>
                {manifest.conflicts.map((c, i) => (
                  <li key={i}>{c}</li>
                ))}
              </ul>
            </div>
          )}

          {error && (
            <div className="conflicts-list" style={{ marginTop: 16 }}>
              ⨯ {error}
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
          >
            Deny
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}
