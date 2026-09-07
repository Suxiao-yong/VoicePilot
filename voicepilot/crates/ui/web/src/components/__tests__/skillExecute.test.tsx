import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import {
  buildAppControlSlots,
  buildGenericSlots,
  executeSkill,
  newExecuteInput,
} from "../../api";
import { SkillExecuteCard } from "../MainView";
import type { Slot } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

function appSlot(raw: string): Slot {
  return { kind: "app", raw, start: 0, end: raw.length, high_risk: false };
}

describe("buildAppControlSlots", () => {
  it("prefers the app slot when text names no app", () => {
    expect(buildAppControlSlots([appSlot("notepad")], "帮我打开它")).toEqual({
      action: "launch",
      app_name: "notepad",
    });
  });

  it("falls back to parsing ASCII app names from text", () => {
    expect(buildAppControlSlots([], "打开应用 notepad")).toEqual({
      action: "launch",
      app_name: "notepad",
    });
  });

  it("infers close/focus actions from text", () => {
    expect(buildAppControlSlots([], "关闭 notepad")?.action).toBe("close");
    expect(buildAppControlSlots([appSlot("calc")], "切换到 calc")?.action).toBe(
      "focus",
    );
  });

  it("resolves Chinese app names via alias (text-only)", () => {
    expect(buildAppControlSlots([], "打开记事本")).toEqual({
      action: "launch",
      app_name: "notepad",
    });
    expect(buildAppControlSlots([], "关闭记事本")?.action).toBe("close");
  });

  it("resolves 飞书 via alias on both slot and text paths", () => {
    expect(buildAppControlSlots([], "打开飞书")).toEqual({
      action: "launch",
      app_name: "Feishu.exe",
    });
    expect(
      buildAppControlSlots(
        [{ kind: "app", raw: "飞书", start: 2, end: 4, high_risk: false }],
        "打开飞书",
      ),
    ).toEqual({ action: "launch", app_name: "Feishu.exe" });
  });

  it("treats slot alias and text ASCII name as the same target", () => {
    expect(
      buildAppControlSlots([appSlot("记事本")], "打开应用 notepad"),
    ).toEqual({ action: "launch", app_name: "notepad" });
  });

  it("returns null when no app name is available", () => {
    expect(buildAppControlSlots([], "你好")).toBeNull();
  });

  it("returns null when slot app and text app differ (fail closed)", () => {
    expect(buildAppControlSlots([appSlot("notepad")], "关闭 calc")).toBeNull();
  });

  it("resolves text-only focus with focus verbs", () => {
    expect(buildAppControlSlots([], "切换到 calc")).toEqual({
      action: "focus",
      app_name: "calc",
    });
  });

  it("resolves multi-word app names via the known-apps table", () => {
    expect(buildAppControlSlots([], "打开 Microsoft Edge")).toEqual({
      action: "launch",
      app_name: "msedge.exe",
    });
    expect(buildAppControlSlots([], "关闭Edge")).toEqual({
      action: "close",
      app_name: "msedge.exe",
    });
  });

  it("does not match ASCII names inside other words", () => {
    expect(buildAppControlSlots([], "Knowledge很好")).toBeNull();
  });
});

describe("newExecuteInput", () => {
  it("builds caller-supplied task/step ids with the given prefix", () => {
    const input = newExecuteInput(
      "quick.app_control",
      { action: "launch", app_name: "notepad" },
      "pet",
    );
    expect(input.skill_id).toBe("quick.app_control");
    expect(input.task_id.startsWith("pet-")).toBe(true);
    expect(input.step_id.startsWith("s-")).toBe(true);
    expect(input.slots_json).toEqual({
      action: "launch",
      app_name: "notepad",
    });
  });
});

describe("executeSkill", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    cleanup();
  });

  it("invokes execute_skill_command with the input wrapper", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: true,
      summary: "quick.app_control 执行成功",
      error: null,
    });
    const input = newExecuteInput("quick.app_control", {
      action: "launch",
      app_name: "notepad",
    });
    const res = await executeSkill(input);
    expect(invoke).toHaveBeenCalledWith("execute_skill_command", { input });
    expect(res.committed).toBe(true);
  });
});

describe("SkillExecuteCard", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    cleanup();
  });

  it("renders skill name + slots and executes with assembled args", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: true,
      summary: "quick.app_control 执行成功",
      error: null,
    });
    const user = userEvent.setup();
    render(
      <SkillExecuteCard
        skillId="quick.app_control"
        slots={[appSlot("notepad")]}
        sourceText="打开应用 notepad"
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByText("quick.app_control")).toBeInTheDocument();
    expect(screen.getByText("app: notepad")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "执行" }));
    expect(invoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = vi.mocked(invoke).mock.calls[0] as [
      string,
      { input: { skill_id: string; slots_json: Record<string, unknown> } },
    ];
    expect(cmd).toBe("execute_skill_command");
    expect(args.input.skill_id).toBe("quick.app_control");
    expect(args.input.slots_json).toEqual({
      action: "launch",
      app_name: "notepad",
    });
    expect(await screen.findByText("执行成功")).toBeInTheDocument();
  });

  it("shows denied state on user denial", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: false,
      summary: "",
      error: "user denied app_control",
    });
    const user = userEvent.setup();
    render(
      <SkillExecuteCard
        skillId="quick.app_control"
        slots={[appSlot("notepad")]}
        sourceText="打开应用 notepad"
        onDismiss={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "执行" }));
    expect(await screen.findByText("已拒绝")).toBeInTheDocument();
  });

  it("shows failure state and resets busy on IPC rejection", async () => {
    // vitest 下无 __TAURI_INTERNALS__，tauriInvoke 把任何 invoke 拒绝都收敛为
    // preview-unavailable 拒绝；此处断言 catch 通路（失败态 + busy 复位），
    // 错误原文透传由下一用例覆盖。
    vi.mocked(invoke).mockRejectedValue(new Error("boom"));
    const user = userEvent.setup();
    render(
      <SkillExecuteCard
        skillId="quick.app_control"
        slots={[appSlot("notepad")]}
        sourceText="打开应用 notepad"
        onDismiss={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "执行" }));
    expect(await screen.findByText("执行失败")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行" })).toBeEnabled();
  });

  it("renders backend error text on resolved failure", async () => {
    vi.mocked(invoke).mockResolvedValue({
      committed: false,
      summary: "",
      error: "boom",
    });
    const user = userEvent.setup();
    render(
      <SkillExecuteCard
        skillId="quick.app_control"
        slots={[appSlot("notepad")]}
        sourceText="打开应用 notepad"
        onDismiss={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "执行" }));
    expect(await screen.findByText("执行失败")).toBeInTheDocument();
    expect(screen.getByText("boom")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行" })).toBeEnabled();
  });

  it("enables execute for non-app_control skills via generic builder", () => {
    render(
      <SkillExecuteCard
        skillId="note.capture"
        slots={[]}
        sourceText="记一条笔记"
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "执行" })).toBeEnabled();
    expect(
      screen.queryByText("该 Skill 暂不支持一键执行"),
    ).not.toBeInTheDocument();
  });

  it("buildGenericSlots maps slot kinds to input fields", () => {
    expect(
      buildGenericSlots([
        {
          kind: "url",
          raw: "https://example.com",
          start: 0,
          end: 19,
          high_risk: false,
        },
      ]),
    ).toEqual({ url: "https://example.com" });
    expect(buildGenericSlots([])).toEqual({});
  });
});
