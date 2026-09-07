import { useState } from "react";
import { cancelVoice, invokeCancelTts } from "../api";
import { Icon } from "../icons";

interface Props {
  /** 窄窗口模式(< 768px)— 显示汉堡按钮(W6a Fast-Follow 响应式) */
  isNarrow: boolean;
  /** 切换 sidebar overlay 开关 */
  onToggleSidebar: () => void;
}

/**
 * KillSwitchBar —— W6b-2 Task 6 紧急停止条(issue #57)+ W6a Fast-Follow 响应式汉堡菜单。
 *
 * 诚实化改造(2026-08-24):当前后端仅有 voice/tts 两个取消通道,无 DAG 取消命令,
 * 故文案不再宣称「停止所有执行」,只做能真正做到的:取消录音 + 停止语音反馈。
 * 真·全局取消(含 DAG)待后端 cancel_* 命令面齐备后再升级。
 */
export function KillSwitchBar({
  isNarrow,
  onToggleSidebar,
}: Props): JSX.Element {
  const [busy, setBusy] = useState(false);

  const handleKill = (): void => {
    if (busy) return;
    setBusy(true);
    Promise.allSettled([cancelVoice(), invokeCancelTts()]).finally(() =>
      setBusy(false),
    );
  };

  return (
    <div
      className="kill-switch-bar"
      role="banner"
      aria-label="停止录音与语音反馈"
    >
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
        className="kill-switch-btn group"
        onClick={handleKill}
        disabled={busy}
        aria-label="停止录音与语音反馈"
      >
        <span className="btn-icon-circle kill-icon-circle">
          <Icon name="stop" />
        </span>
        {busy ? "停止中…" : "停止录音/TTS"}
      </button>
    </div>
  );
}
