import { Fragment, useEffect, useRef, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";
import { DiffViewer } from "./DiffViewer";
import { useDialogA11y } from "../useDialogA11y";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

/**
 * 文件操作审批弹窗 —— Trust Kernel 发出审批请求时弹出。
 * 关闭（Esc / 卸载）视为拒绝，仅已提交决策时跳过。
 */
export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submittedRef = useRef(false);
  const [expandedDiff, setExpandedDiff] = useState<string | null>(null);
  const denyBtnRef = useRef<HTMLButtonElement | null>(null);
  const { approval_request_id, manifest } = payload;
  const fileCount = manifest.sources.length;

  // 4-8:焦点圈禁 + Esc 统一处理;初始焦点给「拒绝」(高危审批不默认放行)
  const dialogRef = useDialogA11y(onDismiss, { focusRef: denyBtnRef });

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    setError(null);
    try {
      await submitApproval(approval_request_id, decision);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Esc 键关闭 —— 卸载时 cleanup effect 自动发送 deny（一次性语义）
  // 4-8:Esc 已由 useDialogA11y 统一处理,此处不再重复注册

  // 卸载时自动拒绝 —— submittedRef 短路：已提交则跳过
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      submitApproval(approval_request_id, "deny").catch(() => {});
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
        aria-labelledby="approval-modal-title"
        tabIndex={-1}
      >
        <div className="dialog-head">
          <h2 id="approval-modal-title" className="dialog-title">
            <span className="dot" aria-hidden="true" />
            文件操作审批
          </h2>
          <span className="badge">E2 · D2 · Local</span>
        </div>
        <div className="dialog-body">
          <div className="approval-stats">
            <div className="stat-cell">
              <div className="k">源文件</div>
              <div className="v">{fileCount}</div>
            </div>
            <div className="stat-cell">
              <div className="k">总大小</div>
              <div className="v">{manifest.total_bytes.toLocaleString()}</div>
            </div>
            <div className="stat-cell">
              <div className="k">冲突</div>
              <div
                className={`v ${manifest.conflicts.length > 0 ? "danger" : ""}`}
              >
                {manifest.conflicts.length}
              </div>
            </div>
          </div>

          <div className="table-wrap">
            <table className="data-table" aria-label="待审批文件清单">
              <thead>
                <tr>
                  <th scope="col">路径</th>
                  <th scope="col">大小</th>
                  <th scope="col">SHA-256</th>
                  <th scope="col">Diff</th>
                </tr>
              </thead>
              <tbody>
                {manifest.sources.map((s) => {
                  const isExpanded = expandedDiff === s.canonical_path;
                  const destPath = `${manifest.destination}/${s.canonical_path.split(/[\\/]/).pop()}`;
                  return (
                    <Fragment key={s.canonical_path}>
                      <tr>
                        <td className="mono cell-path">{s.canonical_path}</td>
                        <td className="mono">{s.size}</td>
                        <td className="mono cell-hash">
                          {s.sha256.slice(0, 16)}…
                        </td>
                        <td>
                          <button
                            type="button"
                            className="btn btn-sm"
                            onClick={() =>
                              setExpandedDiff(
                                isExpanded ? null : s.canonical_path,
                              )
                            }
                            aria-expanded={isExpanded}
                            aria-label={`查看 ${s.canonical_path} 的 Diff`}
                          >
                            {isExpanded ? "收起" : "Diff"}
                          </button>
                        </td>
                      </tr>
                      {isExpanded && (
                        <tr>
                          <td colSpan={4}>
                            <DiffViewer
                              sourcePath={s.canonical_path}
                              destPath={destPath}
                              onClose={() => setExpandedDiff(null)}
                            />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="field field-gap">
            <label className="field-label" htmlFor="approval-destination">
              目标目录
            </label>
            <input
              id="approval-destination"
              className="field-input mono"
              type="text"
              value={manifest.destination}
              readOnly
            />
          </div>

          {manifest.conflicts.length > 0 && (
            <div className="alert alert-warn" role="alert">
              <span className="alert-icon">⚠</span>
              <div>
                {manifest.conflicts.length} 个冲突：
                <ul>
                  {manifest.conflicts.map((c, i) => (
                    <li key={i}>{c}</li>
                  ))}
                </ul>
              </div>
            </div>
          )}

          {error && (
            <div className="alert alert-error" role="alert">
              <span className="alert-icon">⨯</span>
              <span>{error}</span>
            </div>
          )}
        </div>
        <div className="dialog-foot">
          <button
            className="btn btn-danger"
            ref={denyBtnRef}
            onClick={() => decide("deny")}
            disabled={submitting}
          >
            拒绝所有（{fileCount} 个文件）
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            允许所有（{fileCount} 个文件）
          </button>
        </div>
      </div>
    </div>
  );
}
