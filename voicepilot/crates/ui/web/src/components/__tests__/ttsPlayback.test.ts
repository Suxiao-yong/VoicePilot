import { describe, expect, it, vi } from "vitest";
import { stopTtsPlayback } from "../ttsPlayback";

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
