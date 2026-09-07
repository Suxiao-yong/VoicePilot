import { useEffect, useState } from "react";
import {
  checkModel,
  downloadModel,
  isVoiceEnabled,
  onModelDownloadProgress,
} from "../api";
import type { DownloadProgressPayload, ModelStatus } from "../types";

type Phase =
  | "checking"
  | "present"
  | "absent"
  | "downloading"
  | "verifying"
  | "done"
  | "error";

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

/** 后端 ModelStatus → 前端 phase。 */
function statusToPhase(status: ModelStatus): Phase {
  switch (status) {
    case "ready":
      return "present";
    case "missing":
      return "absent";
    case "downloading":
      return "downloading";
    case "verifying":
      return "verifying";
    case "failed":
      return "error";
    case "disabled":
      return "present";
  }
}

/**
 * 语音模型下载横幅 —— 模型缺失时浮出提示,下载时显示进度。
 * voice 未启用或模型已就绪(Ready)时完全不渲染。
 */
export function ModelDownloadBar(): JSX.Element | null {
  const [state, setState] = useState<State>(INITIAL);

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
        setState({
          phase: statusToPhase(status),
          voiceEnabled: true,
          progress: null,
          error: null,
        });
      } catch (e) {
        if (!cancelled) {
          setState({
            phase: "error",
            voiceEnabled: false,
            progress: null,
            error: e instanceof Error ? e.message : String(e),
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (state.phase !== "downloading") return;
    const promise = onModelDownloadProgress((payload) => {
      setState((s) => ({ ...s, progress: payload }));
    });
    return () => {
      promise.then((fn) => fn()).catch(console.error);
    };
  }, [state.phase]);

  // B7:done → present 的 2s 计时器挂到 effect 生命周期,卸载/阶段变化时清理
  useEffect(() => {
    if (state.phase !== "done") return;
    const t = window.setTimeout(() => {
      setState((s) => ({
        ...s,
        phase: "present",
        progress: null,
        error: null,
      }));
    }, 2000);
    return () => window.clearTimeout(t);
  }, [state.phase]);

  if (!state.voiceEnabled || state.phase === "present") return null;

  async function handleDownload(): Promise<void> {
    // 3-3:防连点——下载中(含断点续传入口)忽略重复点击,避免并发发起多个同步下载
    if (state.phase === "downloading") return;
    setState((s) => ({
      ...s,
      phase: "downloading",
      progress: null,
      error: null,
    }));
    try {
      await downloadModel();
      setState((s) => ({ ...s, phase: "done", progress: null, error: null }));
      // 通知 MainView 重新探测模型状态(解锁麦克风按钮)。
      // 定时器改由 done-phase effect 管理(B7),此处只触发一次事件
      window.dispatchEvent(new CustomEvent("voicepilot:model-ready"));
    } catch (e) {
      setState((s) => ({
        ...s,
        phase: "error",
        progress: null,
        error: e instanceof Error ? e.message : String(e),
      }));
    }
  }

  if (state.phase === "checking") {
    return (
      <div className="model-banner" role="status">
        <span className="msg">检查语音模型状态…</span>
      </div>
    );
  }

  if (state.phase === "absent") {
    return (
      <div
        className="model-banner absent"
        role="alert"
        aria-labelledby="model-download-title"
      >
        <span className="status-dot warn pulse" aria-hidden="true" />
        <span id="model-download-title" className="msg">
          语音模型未安装（sherpa-onnx SenseVoice，约 1
          GB），下载后才能使用语音输入
        </span>
        <button
          type="button"
          className="btn btn-primary"
          onClick={handleDownload}
        >
          下载模型
        </button>
      </div>
    );
  }

  if (state.phase === "verifying") {
    return (
      <div className="model-banner" role="status">
        <span className="msg">校验模型归档…</span>
      </div>
    );
  }

  if (state.phase === "downloading") {
    const percent = state.progress?.percent ?? 0;
    const downloadedMb = state.progress
      ? (state.progress.downloaded_bytes / 1024 / 1024).toFixed(1)
      : "0";
    const totalMb = state.progress?.total_bytes
      ? (state.progress.total_bytes / 1024 / 1024).toFixed(1)
      : "?";
    // progress 为 null 表示检测到残留 .part(上次下载中断):提供"继续下载"续传入口。
    const hasResumablePart = state.progress === null;
    return (
      <div className="model-banner downloading" role="status">
        <div className="progress">
          <div
            className="progress-track"
            role="progressbar"
            aria-valuenow={Math.round(percent)}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div className="progress-fill" style={{ width: `${percent}%` }} />
          </div>
          <span className="progress-info">
            {hasResumablePart
              ? "检测到未完成的下载，可断点续传"
              : `下载中…${downloadedMb} / ${totalMb} MB（${percent.toFixed(1)}%）`}
          </span>
        </div>
        {hasResumablePart && (
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleDownload}
          >
            继续下载
          </button>
        )}
      </div>
    );
  }

  if (state.phase === "done") {
    return (
      <div className="model-banner done" role="status">
        <span className="status-dot ok" aria-hidden="true" />
        <span className="msg">下载完成，可以开始使用语音输入</span>
      </div>
    );
  }

  return (
    <div className="model-banner error" role="alert">
      <span className="status-dot bad" aria-hidden="true" />
      <span className="msg">下载失败：{state.error}</span>
      <button type="button" className="btn" onClick={handleDownload}>
        重试
      </button>
    </div>
  );
}
