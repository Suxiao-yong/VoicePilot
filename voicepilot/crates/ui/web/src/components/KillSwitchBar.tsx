import { useState } from "react";
import { cancelVoice } from "../api";

interface Props {
  /** 窄窗口模式(< 768px)— 显示汉堡按钮(W6a Fast-Follow 响应式) */
  isNarrow: boolean;
  /** 切换 sidebar overlay 开关 */
  onToggleSidebar: () => void;
}

/**
 * KillSwitchBar —— W6b-2 Task 6 紧急停止条(issue #57)+ W6a Fast-Follow 响应式汉堡菜单。
 *
 * 顶部条带,提供全局 cancel 按钮触发 `cancel_voice_command`。
 * 窄窗口时左侧加汉堡按钮,点击展开 sidebar overlay。
 */
export function KillSwitchBar({ isNarrow, onToggleSidebar }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);

  const handleKill = (): void => {
    setBusy(true);
    cancelVoice()
      .catch(console.error)
      .finally(() => setBusy(false));
  };

  return (
    <div className="kill-switch-bar" role="banner" aria-label="紧急停止">
      {isNarrow && (
        <button
          type="button"
          className="sidebar-toggle"
          onClick={onToggleSidebar}
          aria-label="切换导航菜单"
        >
          ☰
        </button>
      )}
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
