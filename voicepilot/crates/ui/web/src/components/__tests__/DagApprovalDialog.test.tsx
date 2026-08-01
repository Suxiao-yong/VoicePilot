import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DagApprovalDialog } from "../DagApprovalDialog";
import type { DagApprovalRequestPayload } from "../../types";

// Mock @tauri-apps/api
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { invoke } from "@tauri-apps/api/core";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

const mockPayload: DagApprovalRequestPayload = {
  approval_request_id: "apr_test_001",
  plan_id: "plan_test_001",
  user_goal: "打开记事本写 TODO 然后保存到桌面",
  max_total_steps: 5,
  node_count: 2,
  plan_json: {
    plan_id: "plan_test_001",
    user_goal: "打开记事本写 TODO 然后保存到桌面",
    nodes: [
      {
        node_id: "n1",
        skill_id: "note.capture",
        risk_ceiling: "E1",
        status: "pending",
        input_template_json: JSON.stringify({
          kind: "text",
          template: { kind: "literal", Literal: "notepad" },
        }),
        output_json: null,
        error_message: null,
        task_id: null,
        step_id: null,
        started_at: null,
        completed_at: null,
      },
      {
        node_id: "n2",
        skill_id: "files.move",
        risk_ceiling: "E2",
        status: "pending",
        input_template_json: JSON.stringify({
          kind: "path",
          template: { kind: "var", Var: { scope: "prev", path: "output.path" } },
        }),
        output_json: null,
        error_message: null,
        task_id: null,
        step_id: null,
        started_at: null,
        completed_at: null,
      },
    ],
    edges: [
      { from: "n1", to: "n2", port_binding: "output.path -> input.source" },
    ],
    loop_specs: {},
    max_total_steps: 5,
  },
};

describe("DagApprovalDialog", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders user_goal + node_count + max_total_steps", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    expect(screen.getByText("打开记事本写 TODO 然后保存到桌面")).toBeInTheDocument();
    expect(screen.getByText(/2 个节点/)).toBeInTheDocument();
    expect(screen.getByText(/上限 5 步/)).toBeInTheDocument();
  });

  it("renders node cards with node_id + skill_id + risk badge", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    // n1 / n2 出现在 node card 和 edge list 中,用 getAllByText
    expect(screen.getAllByText("n1").length).toBeGreaterThan(0);
    expect(screen.getByText("note.capture")).toBeInTheDocument();
    expect(screen.getAllByText("n2").length).toBeGreaterThan(0);
    expect(screen.getByText("files.move")).toBeInTheDocument();
    expect(screen.getAllByText("E1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("E2").length).toBeGreaterThan(0);
  });

  it("renders edge list with from -> to", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    expect(screen.getByText("依赖关系(1)")).toBeInTheDocument();
    // edge item 含 n1 → n2
    const edgeItems = screen.getAllByText(/n1/);
    expect(edgeItems.length).toBeGreaterThan(0);
  });

  it("calls approveDagSkeleton with allow when Allow button clicked", async () => {
    const user = userEvent.setup();
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    const allowBtn = screen.getByRole("button", { name: /允许 DAG 执行/i });
    await user.click(allowBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "allow",
      });
    });
  });

  it("calls approveDagSkeleton with deny when Deny button clicked", async () => {
    const user = userEvent.setup();
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    const denyBtn = screen.getByRole("button", { name: /拒绝 DAG 执行/i });
    await user.click(denyBtn);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "deny",
      });
    });
  });

  it("Modify button is enabled and enters editing mode on click", async () => {
    const user = userEvent.setup();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    const modifyBtn = screen.getByRole("button", { name: /调整 DAG 节点/i });
    expect(modifyBtn).not.toBeDisabled();

    await user.click(modifyBtn);

    // 进入编辑模式后,footer 切换为 "提交修改" / "取消"
    expect(screen.getByRole("button", { name: /提交修改后的 DAG/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /取消编辑/i })).toBeInTheDocument();
  });

  it("Esc key triggers onDismiss (which sends deny via cleanup effect)", async () => {
    const onDismiss = vi.fn();
    render(<DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onDismiss).toHaveBeenCalled();
  });

  it("unmount calls approveDagSkeleton with deny (single-use safety)", async () => {
    mockInvoke.mockResolvedValue(true);
    const onDismiss = vi.fn();
    const { unmount } = render(
      <DagApprovalDialog payload={mockPayload} onDismiss={onDismiss} />
    );

    unmount();

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("approve_dag_skeleton_command", {
        approvalRequestId: "apr_test_001",
        decision: "deny",
      });
    });
  });

  it("has dialog role + aria-modal + aria-labelledby", () => {
    render(<DagApprovalDialog payload={mockPayload} onDismiss={() => {}} />);

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("aria-labelledby", "dag-approval-title");
  });
});
