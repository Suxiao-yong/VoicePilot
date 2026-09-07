import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { stopTtsPlayback, selectSpeakText } from "./ttsPlayback";
import {
  routeText,
  organizeFiles,
  executeSkill,
  newExecuteInput,
  buildAppControlSlots,
  buildGenericSlots,
  voiceListen,
  cancelVoice,
  onTranscriptionPartial,
  invokeTts,
  invokeCancelTts,
  isVoiceEnabled,
  checkModel,
  onPetEditText,
} from "../api";

// ponytail: preview-safe wrappers — direct Tauri events/file APIs crash in http://localhost:4173 without __TAURI_INTERNALS__
function safeListen<T>(
  event: string,
  handler: (e: { payload: T }) => void,
): Promise<() => void> {
  try {
    // SAFETY: Tauri 的 listen 在非 Tauri 预览环境下不存在，这里做防御性调用；
    // 类型断言仅用于统一真实 listen 与缺失时的 no-op 签名，调用失败由下方 catch 兜底。
    const p = (
      listen as unknown as (
        e: string,
        h: (ev: { payload: T }) => void,
      ) => Promise<() => void>
    )(event, handler);
    return p.catch(() => () => {});
  } catch {
    return Promise.resolve(() => {});
  }
}
function safeConvertFileSrc(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}
import type {
  RouteTextResult,
  OrganizeResult,
  ExecuteSkillResult,
  VoiceListenResult,
  TranscriptionFinalPayload,
  Slot,
} from "../types";
import { Chip } from "./Chip";
import { SlotEditDialog } from "./SlotEditDialog";
import { Icon } from "../icons";

export function MainView() {
  // ===== 语音输入状态 =====
  const [listening, setListening] = useState(false);
  useEffect(() => {
    window.dispatchEvent(
      new CustomEvent("voicepilot:listening", { detail: listening }),
    );
  }, [listening]);
  const [voiceResult, setVoiceResult] = useState<VoiceListenResult | null>(
    null,
  );
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const [partialText, setPartialText] = useState("");
  // Wave 3 Task 3.2:voice feature 是否启用 + 模型是否就绪(Ready)。
  // voice 未启用 → 隐藏语音按钮;启用但模型未就绪 → 按钮禁用并指向下载横幅。
  const [voiceEnabled, setVoiceEnabled] = useState(false);
  const [modelReady, setModelReady] = useState(false);

  // 启动时探测 voice 能力与模型就绪状态(Task 3.2 UI gating)。
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const enabled = await isVoiceEnabled();
        if (cancelled) return;
        setVoiceEnabled(enabled);
        if (!enabled) return;
        const status = await checkModel();
        if (cancelled) return;
        setModelReady(status === "ready");
      } catch {
        /* 探测失败时按禁用处理(保守) */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Wave 3 Task 3.2:ModelDownloadBanner 下载完成后派发 `voicepilot:model-ready`
  // DOM 事件,这里重查模型状态,让麦克风按钮从"模型未就绪"恢复可用(无需重启)。
  useEffect(() => {
    const handler = (): void => {
      checkModel()
        .then((status) => setModelReady(status === "ready"))
        .catch(() => undefined);
    };
    window.addEventListener("voicepilot:model-ready", handler);
    return () => window.removeEventListener("voicepilot:model-ready", handler);
  }, []);

  // ===== 文本路由状态 =====
  const [text, setText] = useState("");
  const [routeResult, setRouteResult] = useState<RouteTextResult | null>(null);

  // 桌宠化改造:气泡"跳转改字" —— PetWindow 在确认气泡点文字时 emit
  // `pet-edit-text`(pet → main 广播),这里把文字填入 composer 并聚焦,
  // 用户在主界面改完指令再执行。
  const composerInputRef = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    const unlisten = onPetEditText((incoming) => {
      setText(incoming);
      setRouteResult(null);
      setSlots([]);
      // 等一帧让 React 提交新 value 后再聚焦 + 全选,方便直接改字
      requestAnimationFrame(() => {
        composerInputRef.current?.focus();
        composerInputRef.current?.select();
      });
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // ===== 文件整理状态 =====
  const [source, setSource] = useState("");
  const [filter, setFilter] = useState("*.txt");
  const [destination, setDestination] = useState("");
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(
    null,
  );

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // ===== Push-to-talk + TTS =====
  const [pttActive, setPttActive] = useState(false);
  const [ttsPlaying, setTtsPlaying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  // 3-2 录音重入守卫:同步 ref 挡住 mic 按钮与 PTT 快捷键的并发触发
  const listeningRef = useRef(false);

  // 3-1 TTS 切页清理:view 卸载时停止播放并取消后端合成,避免音频残留继续响
  useEffect(() => {
    return () => {
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
      invokeCancelTts().catch(() => {});
    };
  }, []);

  // ===== Slot Chip 修改 =====
  const [slots, setSlots] = useState<Slot[]>([]);
  const [editingSlot, setEditingSlot] = useState<Slot | null>(null);

  // W6b-2 issue #47:监听 partial transcript 事件,实时更新 partialText + slots
  useEffect(() => {
    const unlisten = onTranscriptionPartial((payload) => {
      setPartialText(payload.partial);
      setSlots(payload.slots || []);
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 监听 transcription-final 事件,更新 slots + 清空 partial
  useEffect(() => {
    const unlisten = safeListen<TranscriptionFinalPayload>(
      "transcription-final",
      (event) => {
        setSlots(event.payload.slots || []);
        setPartialText("");
      },
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 监听全局快捷键 Push-to-talk
  useEffect(() => {
    const unlistenStart = safeListen("push-to-talk-start", () => {
      // Wave 3 Task 3.2:快捷键同样受 voice/模型 gating 约束,避免绕过 UI 禁用。
      if (!voiceEnabled || !modelReady) return;
      setPttActive(true);
      onVoiceListen();
    });
    const unlistenStop = safeListen("push-to-talk-stop", () => {
      setPttActive(false);
      cancelVoice().catch(console.error);
    });
    return () => {
      unlistenStart.then((fn) => fn()).catch(() => {});
      unlistenStop.then((fn) => fn()).catch(() => {});
    };
  }, [voiceEnabled, modelReady]);

  async function onVoiceListen() {
    // 3-2:重入守卫——PTT start 事件与 mic 按钮可能相邻触发,只放一个进后端
    if (listeningRef.current) return;
    listeningRef.current = true;
    // Task 7 打断：新一轮录音开始即停掉正在播的 TTS（人一开口音箱就停）。
    // mic 按钮与 PTT 都走本函数，单漏斗全覆盖。
    if (audioRef.current || ttsPlaying) {
      stopTtsPlayback(audioRef.current, setTtsPlaying, invokeCancelTts);
      audioRef.current = null;
    }
    setListening(true);
    setVoiceError(null);
    setVoiceResult(null);
    setPartialText("");
    setSlots([]);
    try {
      const r = await voiceListen();
      setVoiceResult(r);
      // W6b-3b Task 13:转写成功后触发 TTS 语音反馈；聊天兜底播报回答正文。
      if (r.kind === "success" && r.transcription && r.transcription.trim()) {
        const speakText = selectSpeakText(r.transcription, r.route_outcome);
        setTtsPlaying(true);
        try {
          const ttsResult = await invokeTts(speakText);
          if (ttsResult.error) {
            console.warn("TTS error:", ttsResult.error);
            setTtsPlaying(false);
            return;
          }
          // W6b-3b Fix 1:用 wav_path 通过 <audio> 元素播放(convertFileSrc 转 WebView 可访问 URL)
          if (ttsResult.wav_path) {
            const audio = new Audio(safeConvertFileSrc(ttsResult.wav_path));
            audioRef.current = audio;
            audio.onended = () => {
              setTtsPlaying(false);
              audioRef.current = null;
            };
            audio.onerror = () => {
              console.warn("TTS audio playback error");
              setTtsPlaying(false);
              audioRef.current = null;
            };
            await audio.play().catch((e) => {
              console.warn("TTS audio play() rejected:", e);
              setTtsPlaying(false);
              audioRef.current = null;
            });
          } else {
            setTtsPlaying(false);
          }
        } catch (e) {
          console.warn("TTS invoke error:", e);
          setTtsPlaying(false);
        }
      }
    } catch (e) {
      setVoiceError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      listeningRef.current = false;
      setListening(false);
    }
  }

  async function onRoute() {
    setBusy(true);
    setError(null);
    try {
      const r = await routeText(text);
      setRouteResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  // W6c P1 #2:Apply Slot 修改 — 按 slot.end 降序替换 transcription 后重新路由
  async function onApplySlotEdits() {
    const transcription =
      voiceResult?.kind === "success"
        ? voiceResult.transcription
        : voiceResult?.kind === "timeout"
          ? (voiceResult.transcription ?? "")
          : "";
    if (!transcription) return;
    setBusy(true);
    setError(null);
    try {
      const modified = slots
        .filter((s) => s.modified)
        .sort((a, b) => b.end - a.end);
      let newText = transcription;
      for (const slot of modified) {
        newText =
          newText.slice(0, slot.start) + slot.raw + newText.slice(slot.end);
      }
      const r = await routeText(newText);
      setRouteResult(r);
      setSlots((prev) => prev.map((s) => ({ ...s, modified: false })));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  async function onOrganize() {
    setBusy(true);
    setError(null);
    try {
      const r = await organizeFiles({
        task_id: `t-${Date.now()}`,
        step_id: `s-${Date.now()}`,
        source,
        filter,
        destination,
      });
      setOrganizeResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  const handleStopTts = (): void => {
    stopTtsPlayback(audioRef.current, setTtsPlaying, invokeCancelTts);
    audioRef.current = null;
  };

  return (
    <div>
      <header className="view-head">
        <h1 className="view-title">
          <span className="view-kicker">Workbench</span>
          任务工作台
        </h1>
        <p className="view-desc">权限感知 · Trust Kernel 审批 · 语音优先</p>
      </header>

      {/* ===== 输入区 ===== */}
      <div className="composer">
        {/* 4-2:composer 补卡片头,与「文件整理」卡片同构 */}
        <div className="card-head composer-head">
          <div>
            <h2 className="card-title">
              <span className="tick" aria-hidden="true" />
              指令
            </h2>
            <p className="card-desc">
              输入指令或按住语音按钮说话,高风险操作会先经过 Trust Kernel 审批
            </p>
          </div>
        </div>

        <div className="composer-row">
          <input
            className="composer-input"
            type="text"
            value={text}
            ref={composerInputRef}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy && !listening) onRoute();
            }}
            placeholder="例如：整理下载目录，或打开记事本写 TODO…"
            aria-label="指令输入"
          />
          {/* 4-1:主 CTA 提级——与输入框同排的大号 primary,不再缩在 meta 里 */}
          <button
            type="button"
            className="btn btn-primary"
            onClick={onRoute}
            disabled={busy || listening}
          >
            执行指令
          </button>
          {voiceEnabled && (
            <button
              type="button"
              className={`mic-btn ${listening ? "listening" : ""}`}
              onClick={onVoiceListen}
              disabled={listening || busy || !modelReady}
              aria-pressed={listening}
              title={!modelReady ? "语音模型未就绪，请先下载模型" : undefined}
            >
              <span className="mic-icon" aria-hidden="true">
                <Icon name="mic" />
              </span>
              {listening ? "录音中…" : !modelReady ? "模型未就绪" : "语音输入"}
            </button>
          )}
        </div>

        <div className="composer-meta">
          {pttActive && (
            <span className="ptt-hint" role="status" aria-live="polite">
              按住 Ctrl+Alt+Space 录音中…
            </span>
          )}
          {ttsPlaying && (
            <button
              type="button"
              className="btn btn-sm"
              onClick={handleStopTts}
              aria-label="停止语音反馈"
            >
              停止语音反馈
            </button>
          )}
          {listening && (
            <button
              type="button"
              className="btn btn-danger btn-sm"
              onClick={() => cancelVoice().catch(console.error)}
              aria-label="取消录音"
            >
              取消录音
            </button>
          )}
        </div>

        {listening && (
          <div className="rec-indicator" role="status" aria-live="polite">
            <span className="rec-bars" aria-hidden="true">
              <span />
              <span />
              <span />
              <span />
              <span />
            </span>
            <span>录音中，VAD 检测静音后自动停止</span>
            <span className="partial-text">{partialText || "聆听中…"}</span>
          </div>
        )}

        {voiceError && (
          <div className="result-stack">
            <div className="result-card err" role="status" aria-live="polite">
              <div className="result-kicker">错误</div>
              <div className="result-main">{voiceError}</div>
            </div>
          </div>
        )}

        {/* ===== 语音结果 ===== */}
        {voiceResult && (
          <div className="result-stack">
            {voiceResult.kind === "success" && (
              <div className="result-card ok" role="status" aria-live="polite">
                <div className="result-kicker">
                  转写{voiceResult.stopped_by_vad ? " · VAD 自动停止" : ""}
                </div>
                <div className="result-main">{voiceResult.transcription}</div>
                <RouteOutcomeFeedback outcome={voiceResult.route_outcome} />
              </div>
            )}
            {voiceResult.kind === "no_speech" && (
              <div
                className="result-card warn"
                role="status"
                aria-live="polite"
              >
                <div className="result-kicker">结果</div>
                <div className="result-main">未检测到语音</div>
              </div>
            )}
            {voiceResult.kind === "timeout" && (
              <div
                className="result-card warn"
                role="status"
                aria-live="polite"
              >
                <div className="result-kicker">转写 · 超时</div>
                <div className="result-main">
                  {voiceResult.transcription || "（无转写结果）"}
                </div>
                <RouteOutcomeFeedback outcome={voiceResult.route_outcome} />
              </div>
            )}
            {voiceResult.kind === "error" && (
              <div className="result-card err" role="status" aria-live="polite">
                <div className="result-kicker">错误</div>
                <div className="result-main">{voiceResult.message}</div>
              </div>
            )}
          </div>
        )}

        {/* ===== Slot Chips ===== */}
        {slots.length > 0 && (
          <div className="chips" aria-label="可修改参数">
            {slots.map((slot, idx) => (
              <Chip
                key={`${slot.kind}-${slot.start}-${idx}`}
                slot={slot}
                onClick={(s) => setEditingSlot(s)}
              />
            ))}
          </div>
        )}
        {slots.some((s) => s.modified) && (
          <div className="composer-meta">
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={onApplySlotEdits}
              disabled={busy || listening}
              aria-label="Apply 修改并重新路由"
            >
              应用修改并重新路由
            </button>
          </div>
        )}
        <SlotEditDialog
          slot={editingSlot}
          onSubmit={(slot, newValue) => {
            setSlots((prev) =>
              prev.map((s) =>
                s.kind === slot.kind &&
                s.start === slot.start &&
                s.end === slot.end
                  ? { ...s, raw: newValue, modified: true }
                  : s,
              ),
            );
            setEditingSlot(null);
          }}
          onClose={() => setEditingSlot(null)}
        />

        {/* 4-3:无结果时的引导空状态(点击填入 composer) */}
        {!busy &&
          !voiceResult &&
          !routeResult &&
          !error &&
          slots.length === 0 && (
            <div className="empty-state">
              <p className="empty-state-title">试试这样说</p>
              <div className="suggest-list">
                {[
                  "整理下载目录里的 PDF 到文档文件夹",
                  "打开记事本写一条 TODO",
                  "把 D:/A 目录和 D:/B 目录做个对比",
                ].map((s) => (
                  <button
                    key={s}
                    type="button"
                    className="suggest-chip"
                    onClick={() => setText(s)}
                  >
                    {s}
                  </button>
                ))}
              </div>
            </div>
          )}

        {/* ===== 文本路由结果 ===== */}
        {/* Routed 且非 organize → Skill 执行确认卡；其余沿用只读路由卡 */}
        {routeResult &&
        routeResult.kind === "routed" &&
        !routeResult.skill_id.includes("organize") ? (
          <SkillExecuteCard
            key={`${routeResult.skill_id}-${text}-${routeResult.slots.map((s) => `${s.kind}:${s.raw}`).join(",")}`}
            skillId={routeResult.skill_id}
            slots={routeResult.slots}
            sourceText={text}
            onDismiss={() => setRouteResult(null)}
          />
        ) : (
          routeResult && <RouteResultCard result={routeResult} />
        )}
        {error && (
          <div className="alert alert-error" role="alert">
            <span className="alert-icon">⨯</span>
            <span>{error}</span>
          </div>
        )}
      </div>

      {/* ===== 文件整理 ===== */}
      <div className="card card-gap">
        <div className="card-head">
          <div>
            <h2 className="card-title">
              <span className="tick" aria-hidden="true" />
              文件整理
            </h2>
            <p className="card-desc">
              调用 files.organize，按规则移动文件到目标目录
            </p>
          </div>
        </div>
        <div className="field-row">
          <div className="field">
            <label className="field-label" htmlFor="organize-source">
              源目录
            </label>
            <input
              id="organize-source"
              className="field-input mono"
              type="text"
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder="D:/Downloads"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="organize-filter">
              文件规则
            </label>
            <input
              id="organize-filter"
              className="field-input mono"
              type="text"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="*.pdf"
            />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="organize-destination">
              目标目录
            </label>
            <input
              id="organize-destination"
              className="field-input mono"
              type="text"
              value={destination}
              onChange={(e) => setDestination(e.target.value)}
              placeholder="D:/Documents/Papers"
            />
          </div>
          <button
            className="btn btn-primary"
            onClick={onOrganize}
            disabled={busy || listening}
          >
            开始整理
          </button>
        </div>

        {organizeResult && (
          <div className="org-summary">
            <div className="org-stat">
              <div className="k">已提交</div>
              <div className="v">{String(organizeResult.committed)}</div>
            </div>
            <div className="org-stat">
              <div className="k">移动文件</div>
              <div className="v">{organizeResult.moved_paths.length}</div>
            </div>
            <div className="org-stat">
              <div className="k">证据强度</div>
              <div className="v">{organizeResult.evidence_strength}</div>
            </div>
            {organizeResult.compensation_ref && (
              <div className="org-stat">
                <div className="k">补偿引用</div>
                <div className="v mono-sm">
                  {organizeResult.compensation_ref}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** 路由结果卡片 —— 显示 Skill 命中 / 未匹配 / 空输入。 */
export function RouteOutcomeFeedback({
  outcome,
}: {
  outcome: RouteTextResult;
}) {
  return (
    <div className="result-sub">
      {outcome.kind === "routed" && (
        <>
          <span className="pill pill-on">Skill 命中</span>
          <span className="mono-sm">{outcome.skill_id}</span>
        </>
      )}
      {outcome.kind === "unmatched" && (
        <>
          <span className="pill pill-warn">Planner 路径</span>
          <span className="mono-sm">{outcome.text}</span>
        </>
      )}
      {outcome.kind === "chat" && (
        <>
          <span className="pill pill-on">AI 回答</span>
          <span className="mono-sm">{outcome.text}</span>
        </>
      )}
      {outcome.kind === "empty" && <span className="pill">空输入</span>}
    </div>
  );
}

/**
 * Skill 执行确认卡（Skill 执行接线 Phase 2）。
 *
 * Routed 且非 organize 时渲染：skill 名 + slots + 执行/取消。确认调
 * `execute_skill_command`，结果回显成功/拒绝/失败三种态。注意这是第一道门
 * （“我要执行吗”）；需要审批的操作（fs 写/删、shell、form.submit）由
 * trust-kernel 再弹一次审批卡，其余直接执行。
 *
 * slots_json 组装：`quick.app_control` 走专用组装器（含别名归一+动作推断）；
 * 其余 Skill 走通用组装器（slot.kind 即 manifest input 名，直接映射）。
 */
export function SkillExecuteCard({
  skillId,
  slots,
  sourceText,
  onDismiss,
}: {
  skillId: string;
  slots: Slot[];
  sourceText: string;
  onDismiss: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ExecuteSkillResult | null>(null);
  // app_control 走专用组装器（别名归一+动作推断，取不到 app 返回 null）；
  // 其余 Skill 走通用组装器（kind 即 input 名），恒为对象，执行键常开。
  const slotsJson = skillId.includes("app_control")
    ? buildAppControlSlots(slots, sourceText)
    : buildGenericSlots(slots);

  async function onExecute(): Promise<void> {
    if (!slotsJson) return;
    setBusy(true);
    try {
      const r = await executeSkill(newExecuteInput(skillId, slotsJson));
      setResult(r);
    } catch (e) {
      setResult({
        committed: false,
        summary: "",
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setBusy(false);
    }
  }

  const denied =
    result && !result.committed && /denied/i.test(result.error ?? "");
  return (
    <div className="result-stack">
      <div className="result-card ok">
        <div className="result-kicker">确认执行</div>
        <div className="result-main">
          已路由到 Skill：<span className="mono-sm">{skillId}</span>
        </div>
        {slots.length > 0 && (
          <div className="chips" aria-label="识别到的参数">
            {slots.map((slot, idx) => (
              <span
                key={`${slot.kind}-${slot.start}-${idx}`}
                className="mono-sm"
              >
                {slot.kind}: {slot.raw}
              </span>
            ))}
          </div>
        )}
        {slotsJson && (
          <div className="result-sub">
            <span className="mono-sm">{JSON.stringify(slotsJson)}</span>
          </div>
        )}
        <div className="composer-meta">
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={onExecute}
            disabled={busy || !slotsJson}
            title={
              slotsJson
                ? undefined
                : "缺少执行参数：换个说法或去主界面补槽位后重试"
            }
          >
            {busy ? "执行中…" : "执行"}
          </button>
          <button
            type="button"
            className="btn btn-sm"
            onClick={onDismiss}
            disabled={busy}
          >
            取消
          </button>
        </div>
        {!slotsJson && (
          <div className="result-sub">
            <span className="pill pill-warn">缺少执行参数，无法一键执行</span>
          </div>
        )}
        {result && result.committed && (
          <div className="result-sub">
            <span className="pill pill-on">执行成功</span>
            <span className="mono-sm">{result.summary}</span>
          </div>
        )}
        {result && !result.committed && denied && (
          <div className="result-sub">
            <span className="pill pill-warn">已拒绝</span>
            <span className="mono-sm">{result.error}</span>
          </div>
        )}
        {result && !result.committed && !denied && (
          <div className="result-sub">
            <span className="pill pill-warn">执行失败</span>
            <span className="mono-sm">{result.error ?? "未知错误"}</span>
          </div>
        )}
      </div>
    </div>
  );
}

function RouteResultCard({ result }: { result: RouteTextResult }) {
  return (
    <div className="result-stack">
      <div
        className={`result-card ${
          result.kind === "routed"
            ? "ok"
            : result.kind === "empty"
              ? ""
              : "warn"
        }`}
      >
        <div className="result-kicker">路由结果</div>
        <div className="result-main">
          {result.kind === "routed" && <>已路由到 Skill：{result.skill_id}</>}
          {result.kind === "chat" && <>{result.text}</>}
          {result.kind === "unmatched" && <>未匹配到 Skill：{result.text}</>}
          {result.kind === "empty" && <>输入为空</>}
        </div>
      </div>
    </div>
  );
}
