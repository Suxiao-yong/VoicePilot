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
