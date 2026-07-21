import { useState } from "react";
import { cancelVoice } from "../api";

/**
 * KillSwitchBar —— W6b-2 Task 6 紧急停止条(issue #57)。
 *
 * 顶部条带,提供全局 cancel 按钮触发 `cancel_voice_command`。
 * 与 MainView 内的局部取消按钮不同,这个是全局可见的紧急停止。
 */
export function KillSwitchBar(): JSX.Element {
  const [busy, setBusy] = useState(false);

  const handleKill = (): void => {
    setBusy(true);
    cancelVoice()
      .catch(console.error)
      .finally(() => setBusy(false));
  };

  return (
    <div className="kill-switch-bar" role="banner" aria-label="紧急停止">
      <span className="kill-switch-label">⚠ 紧急停止</span>
      <button
        type="button"
        className="kill-switch-btn"
        onClick={handleKill}
        disabled={busy}
        aria-label="停止所有执行"
      >
        {busy ? "停止中…" : "停止所有"}
      </button>
    </div>
  );
}
