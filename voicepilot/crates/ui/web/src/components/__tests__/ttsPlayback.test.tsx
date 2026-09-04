import { describe, expect, it, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { stopTtsPlayback, selectSpeakText } from "../ttsPlayback";
import { RouteOutcomeFeedback } from "../MainView";
import type { RouteTextResult } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((p: string) => p),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("stopTtsPlayback", () => {
  it("pauses audio, cancels backend synthesis, and clears playing flag", () => {
    const pause = vi.fn();
    const setTtsPlaying = vi.fn();
    let cancelCalled = false;
    stopTtsPlayback({ pause }, setTtsPlaying, async () => {
      cancelCalled = true;
    });
    expect(pause).toHaveBeenCalledTimes(1);
    expect(cancelCalled).toBe(true);
    expect(setTtsPlaying).toHaveBeenCalledWith(false);
  });

  it("works when nothing is playing", () => {
    const setTtsPlaying = vi.fn();
    stopTtsPlayback(null, setTtsPlaying, async () => {});
    expect(setTtsPlaying).toHaveBeenCalledWith(false);
  });
});

describe("selectSpeakText", () => {
  it("prefers non-blank chat answer over transcription", () => {
    const outcome: RouteTextResult = { kind: "chat", text: "我是 VoicePilot" };
    expect(selectSpeakText("你好", outcome)).toBe("我是 VoicePilot");
  });

  it("falls back to transcription confirmation for non-chat outcomes", () => {
    const outcome: RouteTextResult = { kind: "unmatched", text: "xxx" };
    expect(selectSpeakText("打开记事本", outcome)).toBe("已为您打开记事本");
  });

  it("falls back when chat text is blank", () => {
    const outcome: RouteTextResult = { kind: "chat", text: "   " };
    expect(selectSpeakText("你好", outcome)).toBe("已为您你好");
  });

  it("falls back when outcome is null", () => {
    expect(selectSpeakText("你好", null)).toBe("已为您你好");
  });
});

describe("RouteOutcomeFeedback", () => {
  it("renders chat answer text", () => {
    render(
      <RouteOutcomeFeedback
        outcome={{ kind: "chat", text: "我是 VoicePilot" }}
      />,
    );
    expect(screen.getByText("我是 VoicePilot")).toBeTruthy();
    expect(screen.getByText("AI 回答")).toBeTruthy();
    cleanup();
  });
});
