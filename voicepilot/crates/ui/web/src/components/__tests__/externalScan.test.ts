import { describe, expect, it, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  importExternalMcp,
  importExternalSkill,
  scanExternalMcp,
  scanExternalSkills,
} from "../../api";
import type { ExternalMcp } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

const CANDIDATE: ExternalMcp = {
  source_file: "C:\\fake\\claude_desktop_config.json",
  sources: ["C:\\fake\\claude_desktop_config.json (claude-desktop)"],
  format: "claude-desktop",
  server_id: "ext-probe",
  name: "Probe",
  command: "npx",
  args: ["-y", "probe"],
  env_keys: ["TOK"],
};

describe("external discovery api", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("scanExternalSkills invokes the scan command with no args", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const res = await scanExternalSkills();
    expect(invoke).toHaveBeenCalledWith(
      "scan_external_skills_command",
      undefined,
    );
    expect(res).toEqual([]);
  });

  it("importExternalSkill passes the directory through", async () => {
    vi.mocked(invoke).mockResolvedValue({
      skill_id: "ext-skill",
      title: "T",
      description: "D",
      executable: false,
      source_path: "C:\\x\\SKILL.md",
    });
    const res = await importExternalSkill("C:\\fake\\ext-skill");
    expect(invoke).toHaveBeenCalledWith("import_external_skill_command", {
      dir: "C:\\fake\\ext-skill",
    });
    expect(res.skill_id).toBe("ext-skill");
  });

  it("scanExternalMcp returns hits plus skipped accounting", async () => {
    vi.mocked(invoke).mockResolvedValue({
      hits: [CANDIDATE],
      skipped: [{ server_id: "old-remote", reason: "unsupported transport" }],
    });
    const res = await scanExternalMcp();
    expect(invoke).toHaveBeenCalledWith("scan_external_mcp_command", undefined);
    expect(res.hits).toHaveLength(1);
    expect(res.hits[0].server_id).toBe("ext-probe");
    expect(res.hits[0].env_keys).toEqual(["TOK"]);
    expect(res.skipped).toHaveLength(1);
  });

  it("importExternalMcp passes only the reference triple back", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await importExternalMcp(
      CANDIDATE.source_file,
      CANDIDATE.format,
      CANDIDATE.server_id,
    );
    // secrets 绝不经过 renderer：只传回引用三元组，值由后端重读。
    expect(invoke).toHaveBeenCalledWith("import_external_mcp_command", {
      source_file: CANDIDATE.source_file,
      format: CANDIDATE.format,
      server_id: CANDIDATE.server_id,
    });
    const [, args] = vi.mocked(invoke).mock.calls[0] as unknown as [
      string,
      Record<string, unknown>,
    ];
    expect(JSON.stringify(args)).not.toContain("TOK");
  });
});

