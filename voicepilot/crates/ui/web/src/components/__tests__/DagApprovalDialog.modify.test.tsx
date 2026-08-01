import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { DagApprovalDialog } from "../DagApprovalDialog";
import type { DagApprovalRequestPayload } from "../../types";

// Mock @tauri-apps/api (与既有 DagApprovalDialog.test.tsx 同模式)
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { invoke } from "@tauri-apps/api/core";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const samplePayload: DagApprovalRequestPayload = {
  approval_request_id: "dag_test_123",
  plan_id: "w9p4-test",
  user_goal: "测试 Modify 路径",
  max_total_steps: 5,
  node_count: 1,
  plan_json: {
    plan_id: "w9p4-test",
    user_goal: "测试 Modify 路径",
    nodes: [
      {
        node_id: "n1",
        skill_id: "note.capture",
        risk_ceiling: "E1",
        status: "pending",
        input_template_json: JSON.stringify({
          kind: "text",
          template: { Literal: "原始" },
        }),
        output_json: null,
        error_message: null,
        task_id: null,
        step_id: null,
        started_at: null,
        completed_at: null,
      },
    ],
    edges: [],
    loop_specs: {},
    max_total_steps: 5,
  },
};

describe("DagApprovalDialog Modify path", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
  });

  it("click_modify_enters_editing_mode", () => {
    render(<DagApprovalDialog payload={samplePayload} onDismiss={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: /调整 DAG 节点/i }));

    expect(screen.getByRole("button", { name: /提交修改后的 DAG/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /取消编辑/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /添加新节点/i })).toBeInTheDocument();
  });

  it("add_node_increases_editedNodes_length", () => {
    render(<DagApprovalDialog payload={samplePayload} onDismiss={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: /调整 DAG 节点/i }));
    fireEvent.click(screen.getByRole("button", { name: /添加新节点/i }));

    const editors = screen.getAllByRole("group", { name: /节点.*编辑器/ });
    expect(editors.length).toBe(2);
  });

  it("delete_node_decreases_editedNodes_length", () => {
    render(<DagApprovalDialog payload={samplePayload} onDismiss={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: /调整 DAG 节点/i }));
    fireEvent.click(screen.getByRole("button", { name: /删除节点 n1/i }));

    const editors = screen.queryAllByRole("group", { name: /节点.*编辑器/ });
    expect(editors.length).toBe(0);
  });

  it("submit_modified_plan_calls_approveDagSkeleton_with_modify", async () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={samplePayload} onDismiss={onDismiss} />);

    // 进入编辑模式
    fireEvent.click(screen.getByRole("button", { name: /调整 DAG 节点/i }));
    // 修改 risk_ceiling(通过 label 查找 select)
    fireEvent.change(screen.getByLabelText("风险上限"), { target: { value: "E2" } });
    // 提交修改
    fireEvent.click(screen.getByRole("button", { name: /提交修改后的 DAG/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "dag_test_123",
        decision: "modify",
        modifiedPlan: expect.objectContaining({
          plan_id: "w9p4-test",
          nodes: expect.arrayContaining([
            expect.objectContaining({ risk_ceiling: "E2" }),
          ]),
        }),
      });
    });
    expect(onDismiss).toHaveBeenCalled();
  });
});
