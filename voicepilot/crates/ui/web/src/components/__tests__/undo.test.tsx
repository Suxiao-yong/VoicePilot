import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { newUndoInput, undoCompensation } from "../../api";
import { UndoButton } from "../UndoButton";
import type { CompensableAction } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

const action: CompensableAction = {
  comp_id: "comp-1",
  step_id: "step-1",
  level: "strong",
  status: "active",
  ttl_expires: "2099-01-01T00:00:00+00:00",
  description: "filesystem.reverse_move（strong）",
};

describe("newUndoInput", () => {
  it("builds a fresh undo task/step targeting the action step", () => {
    const input = newUndoInput(action);
    expect(input.target_step_id).toBe("step-1");
    expect(input.task_id.startsWith("undo-")).toBe(true);
    expect(input.step_id.startsWith("s-undo-")).toBe(true);
  });
});

describe("undoCompensation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes undo_compensation_command with the input wrapper", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: true,
      compensation_status: "reversed",
      summary: "已回滚 step-1 的副作用",
      error: null,
    });
    const input = newUndoInput(action);
    const res = await undoCompensation(input);
    expect(invoke).toHaveBeenCalledWith("undo_compensation_command", { input });
    expect(res.compensation_status).toBe("reversed");
  });
});

describe("UndoButton", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    cleanup();
  });

  it("calls undoCompensation on click and shows the reversed badge", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: true,
      compensation_status: "reversed",
      summary: "已回滚 step-1 的副作用",
      error: null,
    });
    const onDone = vi.fn();
    const user = userEvent.setup();
    render(<UndoButton action={action} onDone={onDone} />);

    await user.click(screen.getByRole("button", { name: "撤销 step-1" }));

    expect(invoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = vi.mocked(invoke).mock.calls[0] as [
      string,
      { input: { target_step_id: string } },
    ];
    expect(cmd).toBe("undo_compensation_command");
    expect(args.input.target_step_id).toBe("step-1");
    expect(await screen.findByText("已补偿回滚")).toBeInTheDocument();
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "撤销 step-1" }),
    ).not.toBeInTheDocument();
  });

  it("shows the backend error text when the undo fails", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: false,
      compensation_status: "failed",
      summary: "",
      error: "no active compensation record found for step step-1",
    });
    const user = userEvent.setup();
    render(<UndoButton action={action} />);

    await user.click(screen.getByRole("button", { name: "撤销 step-1" }));

    expect(
      await screen.findByText(/no active compensation record/),
    ).toBeInTheDocument();
    expect(screen.queryByText("已补偿回滚")).not.toBeInTheDocument();
    // 失败后可重试
    expect(
      screen.getByRole("button", { name: "撤销 step-1" }),
    ).toBeEnabled();
  });

  it("disables the button while the request is in flight", async () => {
    let resolveUndo: ((value: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveUndo = resolve;
        }),
    );
    const user = userEvent.setup();
    render(<UndoButton action={action} />);

    await user.click(screen.getByRole("button", { name: "撤销 step-1" }));
    expect(
      screen.getByRole("button", { name: "撤销 step-1" }),
    ).toBeDisabled();
    expect(screen.getByText("回滚中…")).toBeInTheDocument();

    resolveUndo?.({
      committed: true,
      compensation_status: "reversed",
      summary: "",
      error: null,
    });
    expect(await screen.findByText("已补偿回滚")).toBeInTheDocument();
  });
});
