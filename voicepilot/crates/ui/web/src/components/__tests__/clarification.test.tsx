import { describe, expect, it, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { ClarificationDialog } from "../ClarificationDialog";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

const payload = {
  clarification_request_id: "clf_test",
  question: "搜到多个版本，下载哪一个？",
  options: ["晴天 - 周杰伦", "晴天 (Live)", "晴天 吉他版"],
  default_index: 0,
};

describe("ClarificationDialog", () => {
  it("renders question with one button per option", () => {
    render(<ClarificationDialog payload={payload} onDismiss={() => {}} />);
    expect(
      screen.getByText("搜到多个版本，下载哪一个？"),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(3);
    cleanup();
  });

  it("submits the clicked index and dismisses", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValueOnce(true);
    render(<ClarificationDialog payload={payload} onDismiss={onDismiss} />);
    await user.click(screen.getByRole("button", { name: /Live/ }));
    expect(invoke).toHaveBeenCalledWith("submit_clarification_command", {
      clarificationId: "clf_test",
      selectedIndex: 1,
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
    cleanup();
  });
});
