import { useEffect, useRef, useState } from "react";
import { approveDagSkeleton } from "../api";
import type {
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagEdge,
  DagNode,
} from "../types";

interface Props {
  payload: DagApprovalRequestPayload;
  onDismiss: () => void;
}

/** 从 plan_json 解析 nodes + edges(后端 emit 的 plan_json 是序列化的 DagPlan)。 */
function parsePlanJson(planJson: unknown): { nodes: DagNode[]; edges: DagEdge[] } {
  if (typeof planJson !== "object" || planJson === null) {
    return { nodes: [], edges: [] };
  }
  const plan = planJson as { nodes?: DagNode[]; edges?: DagEdge[] };
  return {
    nodes: plan.nodes ?? [],
    edges: plan.edges ?? [],
  };
}

/** 风险徽章颜色:E0=灰 / E1=蓝 / E2=琥珀 / E3=红(参考 UI 设计要求)。 */
function riskBadgeClass(riskCeiling: string): string {
  const r = riskCeiling.toUpperCase();
  switch (r) {
    case "E0":
      return "risk-badge risk-badge-e0";
    case "E1":
      return "risk-badge risk-badge-e1";
    case "E2":
      return "risk-badge risk-badge-e2";
    case "E3":
      return "risk-badge risk-badge-e3";
    default:
      return "risk-badge risk-badge-e0";
  }
}

/** 渲染 input_template 预览(显示原始模板字符串,前端不渲染模板变量)。 */
function renderTemplatePreview(inputTemplateJson: string): string {
  try {
    const tpl = JSON.parse(inputTemplateJson);
    // SlotTemplate { kind, template: TemplateExpr }
    return JSON.stringify(tpl.template ?? tpl, null, 2);
  } catch {
    return inputTemplateJson;
  }
}

export function DagApprovalDialog({ payload, onDismiss }: Props): JSX.Element {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submittedRef = useRef(false);
  const allowBtnRef = useRef<HTMLButtonElement>(null);
  const {
    approval_request_id,
    plan_id,
    user_goal,
    max_total_steps,
    node_count,
    plan_json,
  } = payload;
  const { nodes, edges } = parsePlanJson(plan_json);

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

  // Esc 键关闭 = Deny(参考 ApprovalModal 模式 + WCAG A 可访问性)
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  // 弹窗打开时聚焦 Allow 按钮(WCAG A:焦点可见 + 键盘可达)
  useEffect(() => {
    allowBtnRef.current?.focus();
  }, []);

  // 卸载时自动 Deny(一次性语义,防 approval_request_id 泄漏后未消费)
  // submittedRef 短路:已提交则跳过
  useEffect(() => {
    return () => {
      if (submittedRef.current) return;
      approveDagSkeleton(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div
        className="modal dag-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dag-approval-title"
      >
        <div className="modal-header">
          <h2 id="dag-approval-title">DAG 骨架审批</h2>
          <span className="badge">
            {node_count} 个节点 · 上限 {max_total_steps} 步
          </span>
        </div>

        <div className="modal-body">
          <div className="dag-goal-section">
            <span id="dag-goal-label" className="dag-label">
              用户意图
            </span>
            <p
              id="dag-goal"
              className="dag-goal-text"
              aria-labelledby="dag-goal-label"
              role="region"
            >
              {user_goal}
            </p>
          </div>

          <div className="dag-plan-section">
            <span className="dag-label">计划 ID</span>
            <code className="dag-plan-id mono">{plan_id}</code>
          </div>

          {/* 节点卡片(minimal bento grid,不嵌套 > 2 层) */}
          <div
            className="dag-nodes-grid"
            role="group"
            aria-label="DAG 节点列表"
          >
            <h3 className="dag-section-title">节点({nodes.length})</h3>
            <div className="dag-nodes-list">
              {nodes.map((node) => (
                <div key={node.node_id} className="dag-node-card">
                  <div className="dag-node-header">
                    <span className="dag-node-id mono">{node.node_id}</span>
                    <span className="dag-node-skill mono">{node.skill_id}</span>
                    <span
                      className={riskBadgeClass(node.risk_ceiling)}
                      aria-label={`风险等级 ${node.risk_ceiling}`}
                    >
                      {node.risk_ceiling}
                    </span>
                  </div>
                  <div className="dag-node-body">
                    <details className="dag-template-preview">
                      <summary className="dag-summary">输入模板</summary>
                      <pre className="dag-template-code mono">
                        {renderTemplatePreview(node.input_template_json)}
                      </pre>
                    </details>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* 边连线图(简化为列表 + 箭头,SVG 拓扑图 W9+ 实现) */}
          {edges.length > 0 && (
            <div className="dag-edges-section">
              <h3 className="dag-section-title">依赖关系({edges.length})</h3>
              <ul className="dag-edges-list" role="list">
                {edges.map((edge, i) => (
                  <li key={i} className="dag-edge-item">
                    <span className="mono">{edge.from}</span>
                    <span className="dag-edge-arrow" aria-hidden="true">
                      →
                    </span>
                    <span className="mono">{edge.to}</span>
                    {edge.port_binding && (
                      <span className="dag-edge-port mono">
                        [{edge.port_binding}]
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {error && (
            <div className="form-error" role="alert">
              错误:{error}
            </div>
          )}
        </div>

        <div className="modal-footer">
          <button
            type="button"
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
            aria-label="拒绝 DAG 执行"
          >
            拒绝(Deny)
          </button>
          <button
            type="button"
            className="btn"
            disabled
            aria-disabled="true"
            title="W9+ 实现"
          >
            调整(Modify)· W9+
          </button>
          <button
            type="button"
            className="btn btn-primary"
            ref={allowBtnRef}
            onClick={() => decide("allow")}
            disabled={submitting}
            aria-label="允许 DAG 执行"
          >
            允许(Allow)
          </button>
        </div>
      </div>
    </div>
  );
}
