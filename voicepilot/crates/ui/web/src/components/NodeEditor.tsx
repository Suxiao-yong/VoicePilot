import { useState } from "react";
import type { DagNode } from "../types";

interface Props {
  node: DagNode;
  onChange(updated: DagNode): void;
  onDelete(): void;
  /** W9 修复(P1-11):JSON 校验结果变化时通知父组件(用于禁用提交按钮)。 */
  onValidityChange?(valid: boolean): void;
}

/** W9 Plan 4:单节点编辑器(供 DagApprovalDialog 编辑模式使用)。 */
export function NodeEditor({ node, onChange, onDelete, onValidityChange }: Props): JSX.Element {
  const [invalid, setInvalid] = useState(false);

  const handleTemplateChange = (value: string): void => {
    const updated = { ...node, input_template_json: value };
    onChange(updated);
    try {
      JSON.parse(value);
      setInvalid(false);
      onValidityChange?.(true);
    } catch {
      setInvalid(true);
      onValidityChange?.(false);
    }
  };

  return (
    <div className="dag-node-editor" role="group" aria-label={`节点 ${node.node_id} 编辑器`}>
      <div className="dag-node-editor-header">
        <label htmlFor={`node-id-${node.node_id}`} className="dag-label">
          节点 ID
        </label>
        <input
          id={`node-id-${node.node_id}`}
          type="text"
          className="mono"
          value={node.node_id}
          readOnly
          aria-readonly="true"
        />
        <button
          type="button"
          className="btn btn-danger dag-delete-node-btn"
          onClick={onDelete}
          aria-label={`删除节点 ${node.node_id}`}
        >
          删除节点
        </button>
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`skill-id-${node.node_id}`} className="dag-label">
          Skill ID
        </label>
        <input
          id={`skill-id-${node.node_id}`}
          type="text"
          className="mono"
          value={node.skill_id}
          onChange={(e) => onChange({ ...node, skill_id: e.target.value })}
        />
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`risk-ceiling-${node.node_id}`} className="dag-label">
          风险上限
        </label>
        <select
          id={`risk-ceiling-${node.node_id}`}
          value={node.risk_ceiling}
          onChange={(e) => onChange({ ...node, risk_ceiling: e.target.value })}
        >
          <option value="E0">E0(无风险)</option>
          <option value="E1">E1(低风险)</option>
          <option value="E2">E2(中风险)</option>
          <option value="E3">E3(高风险)</option>
        </select>
      </div>

      <div className="dag-node-editor-field">
        <label htmlFor={`input-template-${node.node_id}`} className="dag-label">
          输入模板(JSON)
        </label>
        <textarea
          id={`input-template-${node.node_id}`}
          className={`mono dag-template-textarea${invalid ? " dag-template-invalid" : ""}`}
          rows={6}
          value={node.input_template_json}
          onChange={(e) => handleTemplateChange(e.target.value)}
          aria-label={`节点 ${node.node_id} 输入模板`}
          aria-invalid={invalid}
          spellCheck={false}
        />
        {invalid && (
          <span className="dag-template-error" role="alert">
            Invalid JSON
          </span>
        )}
      </div>
    </div>
  );
}
