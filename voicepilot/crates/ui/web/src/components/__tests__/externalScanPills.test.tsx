import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { SkillsManagerView } from "../SkillsManagerView";
import { TrustCenterView } from "../TrustCenterView";
import type { ExternalMcp, ExternalSkill } from "../../types";

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

const SKILL_HIT: ExternalSkill = {
  source: "claude-code",
  sources: ["claude-code", "codex"],
  dir: "C:\\fake\\ext-skill",
  id: "ext-skill",
  title: "T",
  description: "D",
  executable: false,
  exec_server: null,
  exec_tool: null,
};

const MCP_HIT: ExternalMcp = {
  source_file: "C:\\fake\\claude_desktop_config.json",
  sources: ["C:\\fake\\claude_desktop_config.json (claude-desktop)"],
  format: "claude-desktop",
  server_id: "ext-probe",
  name: "Probe",
  command: "npx",
  args: ["-y", "probe"],
  env_keys: ["TOK"],
};

describe("external discovery pills", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    cleanup();
  });

  function mockInvoke(opts: {
    userSkills?: { skill_id: string }[];
    servers?: { server_id: string }[];
  }): void {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      switch (cmd) {
        case "list_skills_command":
          return [];
        case "list_user_skills_command":
          return opts.userSkills ?? [];
        case "scan_external_skills_command":
          return [SKILL_HIT];
        case "list_mcp_servers_command":
          return opts.servers ?? [];
        case "scan_external_mcp_command":
          return { hits: [MCP_HIT], skipped: [] };
        default:
          return [];
      }
    });
  }

  it("skills view marks installed vs not-installed by skill_id", async () => {
    const user = userEvent.setup();
    mockInvoke({ userSkills: [] });
    render(<SkillsManagerView />);
    await user.click(
      screen.getByRole("button", { name: /扫描全局第三方 Skill/ }),
    );
    expect(await screen.findByText("未导入")).toBeInTheDocument();
    // Phase A 去重：同 id 同内容多来源合并显示来源列表（空白归一后匹配）。
    expect(await screen.findByText(/claude-code\s+codex/)).toBeInTheDocument();
    cleanup();

    mockInvoke({ userSkills: [{ skill_id: "ext-skill" }] });
    render(<SkillsManagerView />);
    await user.click(
      screen.getByRole("button", { name: /扫描全局第三方 Skill/ }),
    );
    expect(await screen.findByText("已导入")).toBeInTheDocument();
  });

  it("trust center marks installed vs not-installed by server_id", async () => {
    const user = userEvent.setup();
    mockInvoke({ servers: [] });
    render(<TrustCenterView />);
    await user.click(
      screen.getByRole("button", { name: /扫描全局第三方 MCP/ }),
    );
    expect(await screen.findByText("未导入")).toBeInTheDocument();
    cleanup();

    mockInvoke({ servers: [{ server_id: "ext-probe" }] });
    render(<TrustCenterView />);
    await user.click(
      screen.getByRole("button", { name: /扫描全局第三方 MCP/ }),
    );
    expect(await screen.findByText("已导入")).toBeInTheDocument();
  });

  it("trust center shows the skipped line when entries are skipped", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "list_mcp_servers_command") return [];
      if (cmd === "scan_external_mcp_command") {
        return {
          hits: [],
          skipped: [
            { server_id: "old-remote", reason: "unsupported transport" },
          ],
        };
      }
      return [];
    });
    render(<TrustCenterView />);
    await user.click(
      screen.getByRole("button", { name: /扫描全局第三方 MCP/ }),
    );
    expect(await screen.findByText(/跳过 1 个/)).toBeInTheDocument();
  });
});
