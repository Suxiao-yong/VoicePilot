import { useEffect, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const { approval_request_id, manifest } = payload;

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    try {
      await submitApproval(approval_request_id, decision);
      onDismiss();
    } catch (e) {
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

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
      <div className="modal">
        <div className="modal-header">
          <h2>Approve File Operation</h2>
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
            <label>Destination</label>
            <input type="text" value={manifest.destination} readOnly />
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
