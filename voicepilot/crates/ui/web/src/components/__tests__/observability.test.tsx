import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { AuditViewerView } from "../AuditViewerView";
import type { ObservabilitySnapshot } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

const snapshot: ObservabilitySnapshot = {
  window_hours: 24,
  routes: [
    {
      route_kind: "fastroute",
      sample_count: 20,
      p50_ms: 2,
      p95_ms: 6,
      p99_ms: 8,
      max_ms: 9,
    },
    {
      route_kind: "keyword",
      sample_count: 10,
      p50_ms: 5,
      p95_ms: 12,
      p99_ms: 14,
      max_ms: 15,
    },
    {
      route_kind: "llm",
      sample_count: 3,
      p50_ms: 820,
      p95_ms: 1500,
      p99_ms: 1600,
      max_ms: 1600,
    },
    {
      route_kind: "cache",
      sample_count: 9,
      p50_ms: 1,
      p95_ms: 2,
      p99_ms: 3,
      max_ms: 3,
    },
  ],
  cache: { hits: 9, misses: 3, hit_rate: 0.75 },
  dag_tokens: { plans: 2, total_tokens: 700, avg_tokens: 350 },
  audit_chain: { valid: true, checked: 42, broken_at: null },
};

function mockBackend(overrides: Partial<ObservabilitySnapshot> = {}): void {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    switch (cmd) {
      case "get_observability_command":
        return Promise.resolve({ ...snapshot, ...overrides });
      case "list_compensable_actions_command":
        return Promise.resolve([
          {
            comp_id: "comp-1",
            step_id: "step-1",
            level: "strong",
            status: "active",
            ttl_expires: "2099-01-01T00:00:00+00:00",
            description: "filesystem.reverse_move（strong）",
          },
        ]);
      case "list_audit_recent_command":
        return Promise.resolve([]);
      case "list_audit_for_task_command":
        return Promise.resolve([]);
      default:
        return Promise.reject(new Error(`unexpected command: ${cmd}`));
    }
  });
}

describe("AuditViewerView 可观测面板", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    cleanup();
  });

  it("renders P50/P95 per route kind and the cache hit rate", async () => {
    mockBackend();
    render(<AuditViewerView />);

    // 快路由 / 关键词 / LLM 三张延迟卡
    expect(await screen.findByText("2 ms / 6 ms")).toBeInTheDocument();
    expect(screen.getByText("5 ms / 12 ms")).toBeInTheDocument();
    expect(screen.getByText("820 ms / 1500 ms")).toBeInTheDocument();
    // 缓存命中率 + 审计链完整性
    expect(screen.getByText("75.0%")).toBeInTheDocument();
    expect(screen.getByText(/完整（42 条）/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_observability_command", {
      windowHours: undefined,
    });
  });

  it("shows placeholders when a route kind has no samples", async () => {
    mockBackend({
      routes: [],
      cache: { hits: 0, misses: 0, hit_rate: 0 },
    });
    render(<AuditViewerView />);

    expect(await screen.findByText("0.0%")).toBeInTheDocument();
    // 三张延迟卡都是 "— / —"
    expect(screen.getAllByText("— / —")).toHaveLength(3);
  });

  it("lists compensable actions with an undo entry", async () => {
    mockBackend();
    render(<AuditViewerView />);

    expect(await screen.findByText("filesystem.reverse_move（strong）"))
      .toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "撤销 step-1" }),
    ).toBeInTheDocument();
  });
});
