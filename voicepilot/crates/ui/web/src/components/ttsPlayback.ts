import type { RouteTextResult } from "../types";

/** 停止 TTS 播放并取消后端合成。自动打断与手动停止共用。 */
export function stopTtsPlayback(
  audio: { pause: () => void } | null,
  setTtsPlaying: (v: boolean) => void,
  cancelTts: () => Promise<unknown>,
): void {
  if (audio) audio.pause();
  cancelTts().catch(() => {});
  setTtsPlaying(false);
}

/** 语音播报选文:聊天兜底播报回答正文(非空时),否则播报转写确认语。纯函数。 */
export function selectSpeakText(
  transcription: string,
  routeOutcome: RouteTextResult | null | undefined,
): string {
  if (routeOutcome?.kind === "chat" && routeOutcome.text.trim()) {
    return routeOutcome.text;
  }
  return `已为您${transcription}`;
}
