import { useEffect, useState } from "react";
import { checkModel, downloadModel, isVoiceEnabled, onModelDownloadProgress } from "../api";
import type { DownloadProgressPayload, ModelStatus } from "../types";

type Phase = "checking" | "present" | "absent" | "downloading" | "done" | "error";

interface State {
  phase: Phase;
  voiceEnabled: boolean;
  progress: DownloadProgressPayload | null;
  error: string | null;
}

const INITIAL: State = {
  phase: "checking",
  voiceEnabled: false,
  progress: null,
  error: null,
};

export function ModelDownloadBar(): JSX.Element | null {
  const [state, setState] = useState<State>(INITIAL);

  // 启动时检测 voice 是否启用 + 模型是否存在
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const enabled = await isVoiceEnabled();
        if (cancelled) return;
        if (!enabled) {
          setState({ ...INITIAL, phase: "present", voiceEnabled: false });
          return;
        }
        const status: ModelStatus = await checkModel();
        if (cancelled) return;
        if (status === "present") {
          setState({ phase: "present", voiceEnabled: true, progress: null, error: null });
        } else {
          setState({ phase: "absent", voiceEnabled: true, progress: null, error: null });
        }
      } catch (e) {
        if (!cancelled) {
          setState({
            phase: "error",
            voiceEnabled: false,
            progress: null,
            error: String(e),
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 监听下载进度
  useEffect(() => {
    if (state.phase !== "downloading") return;
    let unlistenFn: (() => void) | null = null;
    const promise = onModelDownloadProgress((payload) => {
      setState((s) => ({ ...s, progress: payload }));
    });
    promise
      .then((fn) => {
        unlistenFn = fn;
      })
      .catch(() => {});
    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, [state.phase]);

  // voice 未启用 → 不渲染
  if (!state.voiceEnabled) return null;
  // 模型已存在 → 不渲染(无需打扰用户)
  if (state.phase === "present" || state.phase === "done") return null;

  async function handleDownload() {
    setState((s) => ({ ...s, phase: "downloading", progress: null, error: null }));
    try {
      await downloadModel();
      setState((s) => ({ ...s, phase: "done", progress: null, error: null }));
    } catch (e) {
      setState((s) => ({
        ...s,
        phase: "error",
        progress: null,
        error: e instanceof Error ? e.message : String(e),
      }));
    }
  }

  function handleRetry() {
    setState((s) => ({ ...s, phase: "absent", progress: null, error: null }));
  }

  // checking → 简短 loading
  if (state.phase === "checking") {
    return (
      <div className="model-download-bar checking" role="status" aria-live="polite">
        <span>检查语音模型状态…</span>
      </div>
    );
  }

  // absent → 询问用户是否下载
  if (state.phase === "absent") {
    return (
      <div className="model-download-bar absent" role="alertdialog" aria-labelledby="mdl-title">
        <span id="mdl-title" className="mdl-message">
          ⚠ 语音模型未安装(ggml-tiny.bin,~75MB),需要下载后才能使用语音输入。
        </span>
        <button
          type="button"
          className="btn btn-primary mdl-btn"
          onClick={handleDownload}
        >
          下载模型
        </button>
      </div>
    );
  }

  // downloading → 进度条
  if (state.phase === "downloading") {
    const percent = state.progress?.percent ?? 0;
    const downloadedMb = state.progress
      ? (state.progress.downloaded_bytes / 1024 / 1024).toFixed(1)
      : "0";
    const totalMb = state.progress?.total_bytes
      ? (state.progress.total_bytes / 1024 / 1024).toFixed(1)
      : "?";
    return (
      <div className="model-download-bar downloading" role="status" aria-live="polite">
        <div className="mdl-progress-info">
          下载中…{downloadedMb} / {totalMb} MB({percent.toFixed(1)}%)
        </div>
        <div
          className="mdl-progress-bar"
          role="progressbar"
          aria-valuenow={Math.round(percent)}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div className="mdl-progress-fill" style={{ width: `${percent}%` }} />
        </div>
      </div>
    );
  }

  // error → 错误信息 + 重试
  if (state.phase === "error") {
    return (
      <div className="model-download-bar error" role="alert">
        <span className="mdl-message">⨯ 下载失败:{state.error}</span>
        <button
          type="button"
          className="btn btn-secondary mdl-btn"
          onClick={handleRetry}
        >
          重试
        </button>
      </div>
    );
  }

  return null;
}
