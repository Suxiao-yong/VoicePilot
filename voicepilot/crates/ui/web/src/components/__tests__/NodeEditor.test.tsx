import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { NodeEditor } from "../NodeEditor";
import type { DagNode } from "../../types";

const sampleNode: DagNode = {
  node_id: "n1",
  skill_id: "note.capture",
  risk_ceiling: "E1",
  status: "pending",
  input_template_json: JSON.stringify({
    kind: "text",
    template: { Literal: "原始 TODO" },
  }),
  output_json: null,
  error_message: null,
  task_id: null,
  step_id: null,
  started_at: null,
  completed_at: null,
};

describe("NodeEditor", () => {
  it("renders_node_fields_correctly", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    expect(screen.getByDisplayValue("n1")).toBeInTheDocument();
    expect(screen.getByDisplayValue("note.capture")).toBeInTheDocument();
    // select 通过 label 查找,value="E1" 的 option 被选中
    const riskSelect = screen.getByLabelText("风险上限") as HTMLSelectElement;
    expect(riskSelect.value).toBe("E1");
    expect(screen.getByText(/原始 TODO/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /删除节点 n1/ })).toBeInTheDocument();
  });

  it("delete_node_calls_onDelete", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    fireEvent.click(screen.getByRole("button", { name: /删除节点 n1/ }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("change_risk_ceiling_calls_onChange", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    const select = screen.getByLabelText("风险上限");
    fireEvent.change(select, { target: { value: "E2" } });
    expect(onChange).toHaveBeenCalledWith({ ...sampleNode, risk_ceiling: "E2" });
  });

  it("change_input_template_calls_onChange", () => {
    const onChange = vi.fn();
    const onDelete = vi.fn();
    render(
      <NodeEditor node={sampleNode} onChange={onChange} onDelete={onDelete} />
    );

    const textarea = screen.getByRole("textbox", { name: /节点 n1 输入模板/ });
    fireEvent.change(textarea, {
      target: { value: '{"kind":"text","template":{"Literal":"新值"}}' },
    });
    expect(onChange).toHaveBeenCalledWith({
      ...sampleNode,
      input_template_json: '{"kind":"text","template":{"Literal":"新值"}}',
    });
  });
});
