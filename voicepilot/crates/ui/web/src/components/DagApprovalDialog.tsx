import { useEffect, useRef, useState } from "react";
import { approveDagSkeleton } from "../api";
import type {
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagNode,
} from "../types";
import { NodeEditor } from "./NodeEditor";
import { useDialogA11y } from "../useDialogA11y";

interface Props {
  payload: DagApprovalRequestPayload;
  onDismiss: () => void;
}

/** 风险徽章等级类名（E0=灰 / E1=蓝 / E2=琥珀 / E3=红）。 */
function riskClass(riskCeiling: string): string {
  const r = riskCeiling.toUpperCase();
  if (r === "E1" || r === "E2" || r === "E3")
    return `risk-badge risk-${r.toLowerCase()}`;
  return "risk-badge risk-e0";
}

/** 渲染 input_template 预览（显示原始模板字符串，前端不渲染模板变量）。 */
function renderTemplatePreview(inputTemplateJson: string): string {
  try {
    const tpl = JSON.parse(inputTemplateJson);
    return JSON.stringify(tpl.template ?? tpl, null, 2);
  } catch {
    return inputTemplateJson;
  }
}

export function DagApprovalDialog({ payload, onDismiss }: Props): JSX.Element {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingMode, setEditingMode] = useState(false);
  const [editedNodes, setEditedNodes] = useState<DagNode[]>(
    () => payload.plan_json.nodes,
  );
  const [invalidNodeIds, setInvalidNodeIds] = useState<Set<string>>(new Set());
  const submittedRef = useRef(false);
  const denyBtnRef = useRef<HTMLButtonElement>(null);
  // 4-8:焦点圈禁 + Esc 统一处理;初始焦点给 Deny(高危审批盲按 Enter 不放行)
  const dialogRef = useDialogA11y(onDismiss, { focusRef: denyBtnRef });
  const {
    approval_request_id,
    plan_id,
    user_goal,
    max_total_steps,
    node_count,
    plan_json,
  } = payload;
  const { nodes, edges } = plan_json;

  async function decide(decision: DagApprovalDecision): Promise<void> {
    setSubmitting(true);
    setError(null);
    try {
      await approveDagSkeleton(approval_request_id, decision);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  /** 提交 Modify 决策 + 编辑后的节点列表。 */
  async function handleModifySubmit(): Promise<void> {
    setSubmitting(true);
    setError(null);
    try {
      const modifiedPlan = {
        plan_id,
        user_goal,
        nodes: editedNodes,
        edges,
        loop_specs: {},
        max_total_steps,
      };
      await approveDagSkeleton(approval_request_id, "modify", modifiedPlan);
      submittedRef.current = true;
      onDismiss();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Esc 键关闭 = Deny（卸载时 cleanup effect 自动发送 deny）
  // 4-8:Esc 已由 useDialogA11y 统一处理,此处不再重复注册

  // 卸载时自动 Deny（一次性语义，防 approval_request_id 泄漏后未消费）
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      approveDagSkeleton(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="dialog-backdrop">
      <div
        ref={dialogRef}
        className="dialog wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dag-approval-title"
        tabIndex={-1}
      >
        <div className="dialog-head">
          <h2 id="dag-approval-title" className="dialog-title">
            <span className="dot" aria-hidden="true" />
            DAG 骨架审批
          </h2>
          <span className="badge">
            {node_count} 个节点 · 上限 {max_total_steps} 步
          </span>
        </div>

        <div className="dialog-body">
          <div className="dag-goal">
            <span className="k" id="dag-goal-label">
              用户意图
            </span>
            <p id="dag-goal" aria-labelledby="dag-goal-label" role="region">
              {user_goal}
            </p>
          </div>

          <div className="field-row">
            <div className="field">
              <span className="field-label">计划 ID</span>
              <code className="mono">{plan_id}</code>
            </div>
          </div>

          <h3 className="section-title">
            节点({editingMode ? editedNodes.length : nodes.length})
          </h3>
          <div className="node-grid" role="group" aria-label="DAG 节点列表">
            {!editingMode
              ? nodes.map((node) => (
                  <div key={node.node_id} className="node-card">
                    <div className="node-head">
                      <span className="node-id">{node.node_id}</span>
                      <span className="node-skill">{node.skill_id}</span>
                      <span
                        className={riskClass(node.risk_ceiling)}
                        aria-label={`风险等级 ${node.risk_ceiling}`}
                      >
                        {node.risk_ceiling}
                      </span>
                    </div>
                    <details className="tpl-toggle">
                      <summary>输入模板</summary>
                      <pre className="tpl-pre">
                        {renderTemplatePreview(node.input_template_json)}
                      </pre>
                    </details>
                  </div>
                ))
              : editedNodes.map((node) => (
                  <NodeEditor
                    key={node.node_id}
                    node={node}
                    onChange={(updated) =>
                      setEditedNodes((prev) =>
                        prev.map((n) =>
                          n.node_id === updated.node_id ? updated : n,
                        ),
                      )
                    }
                    onDelete={() =>
                      setEditedNodes((prev) =>
                        prev.filter((n) => n.node_id !== node.node_id),
                      )
                    }
                    onValidityChange={(valid) =>
                      setInvalidNodeIds((prev) => {
                        const next = new Set(prev);
                        if (valid) next.delete(node.node_id);
                        else next.add(node.node_id);
                        return next;
                      })
                    }
                  />
                ))}
          </div>
          {editingMode && (
            <button
              type="button"
              className="btn btn-sm add-node-btn"
              onClick={() =>
                setEditedNodes((prev) => [
                  ...prev,
                  {
                    node_id: `n${prev.length + 1}_${Date.now()}`,
                    skill_id: "note.capture",
                    risk_ceiling: "E1",
                    status: "pending" as const,
                    input_template_json: JSON.stringify({
                      kind: "text",
                      template: { Literal: "" },
                    }),
                    output_json: null,
                    error_message: null,
                    task_id: null,
                    step_id: null,
                    started_at: null,
                    completed_at: null,
                  },
                ])
              }
              aria-label="添加新节点"
            >
              + 添加节点
            </button>
          )}

          {edges.length > 0 && (
            <>
              <h3 className="section-title">依赖关系({edges.length})</h3>
              <ul className="edge-list" role="list">
                {edges.map((edge, i) => (
                  <li key={i} className="edge-item">
                    <span className="mono">{edge.from}</span>
                    <span className="edge-arrow" aria-hidden="true">
                      →
                    </span>
                    <span className="mono">{edge.to}</span>
                    {edge.port_binding && (
                      <span className="edge-port">[{edge.port_binding}]</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}

          {error && (
            <div className="alert alert-error" role="alert">
              <span className="alert-icon">⨯</span>
              <span>{error}</span>
            </div>
          )}
        </div>

        <div className="dialog-foot">
          {!editingMode ? (
            <>
              <button
                type="button"
                className="btn btn-danger"
                ref={denyBtnRef}
                onClick={() => decide("deny")}
                disabled={submitting}
                aria-label="拒绝 DAG 执行"
              >
                拒绝（Deny）
              </button>
              <button
                type="button"
                className="btn"
                onClick={() => setEditingMode(true)}
                disabled={submitting}
                aria-label="调整 DAG 节点"
              >
                调整（Modify）
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => decide("allow")}
                disabled={submitting}
                aria-label="允许 DAG 执行"
              >
                允许（Allow）
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                className="btn"
                onClick={() => setEditingMode(false)}
                disabled={submitting}
                aria-label="取消编辑"
              >
                取消
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleModifySubmit}
                disabled={submitting || invalidNodeIds.size > 0}
                aria-label="提交修改后的 DAG"
              >
                提交修改
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
