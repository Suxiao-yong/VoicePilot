import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { routeText, organizeFiles, voiceListen, cancelVoice, onTranscriptionPartial, invokeTts, invokeCancelTts } from "../api";
import type {
  RouteTextResult,
  OrganizeResult,
  VoiceListenResult,
  TranscriptionFinalPayload,
  Slot,
} from "../types";
import { Chip } from "./Chip";
import { SlotEditDialog } from "./SlotEditDialog";

export function MainView() {
  // ===== 语音输入状态(W6b-1) =====
  const [listening, setListening] = useState(false);
  const [voiceResult, setVoiceResult] = useState<VoiceListenResult | null>(null);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  // W6b-2 issue #47:partial transcript 实时显示
  const [partialText, setPartialText] = useState<string>("");

  // ===== route_text 状态(W6a) =====
  const [text, setText] = useState("");
  const [routeResult, setRouteResult] = useState<RouteTextResult | null>(null);

  // ===== organize_files 状态(W6a) =====
  const [source, setSource] = useState("");
  const [filter, setFilter] = useState("*.txt");
  const [destination, setDestination] = useState("");
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(null);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // W6b-3b Task 13:Push-to-talk + TTS 播放状态
  const [pttActive, setPttActive] = useState(false);
  const [ttsPlaying, setTtsPlaying] = useState(false);
  // W6b-3b Fix 1:audioRef 跟踪当前播放的 <audio> 元素,"停止语音反馈"按钮调用 pause() 中断。
  const audioRef = useRef<HTMLAudioElement | null>(null);

  // W6b-3b Task 17:Slot Chip 修改状态(§8.4)
  const [slots, setSlots] = useState<Slot[]>([]);
  const [editingSlot, setEditingSlot] = useState<Slot | null>(null);

  // W6b-2 issue #47:监听 partial transcript 事件,实时更新 partialText
  useEffect(() => {
    const unlisten = onTranscriptionPartial((payload) => {
      setPartialText(payload.partial);
      // W6b-3b Task 17:更新 slots(§8.4 Chip 修改)
      setSlots(payload.slots || []);
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // W6b-3b Task 17:监听 transcription-final 事件,更新 slots + 清空 partial
  useEffect(() => {
    const unlisten = listen<TranscriptionFinalPayload>(
      "transcription-final",
      (event) => {
        setSlots(event.payload.slots || []);
      }
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // W6b-3b Task 13:监听全局快捷键 Push-to-talk 事件
  useEffect(() => {
    const unlistenStart = listen("push-to-talk-start", () => {
      setPttActive(true);
      // 触发 voice listen
      onVoiceListen();
    });
    const unlistenStop = listen("push-to-talk-stop", () => {
      setPttActive(false);
      // 取消 voice listen
      cancelVoice().catch(console.error);
    });
    return () => {
      unlistenStart.then((fn) => fn()).catch(() => {});
      unlistenStop.then((fn) => fn()).catch(() => {});
    };
  }, []);

  async function onVoiceListen() {
    setListening(true);
    setVoiceError(null);
    setVoiceResult(null);
    setPartialText("");
    setSlots([]);
    try {
      const r = await voiceListen();
      setVoiceResult(r);
      // W6b-3b Task 13:转写成功后触发 TTS 语音反馈
      if (r.kind === "success" && r.transcription && r.transcription.trim()) {
        setTtsPlaying(true);
        try {
          const ttsResult = await invokeTts(`已为您${r.transcription}`);
          if (ttsResult.error) {
            console.warn("TTS error:", ttsResult.error);
            setTtsPlaying(false);
            return;
          }
          // W6b-3b Fix 1:用 wav_path 通过 <audio> 元素播放(此前桩实现返回 played: true 但无声音)。
          // convertFileSrc 把文件路径转为 WebView 可访问的 URL(Tauri 2 asset protocol)。
          if (ttsResult.wav_path) {
            const url = convertFileSrc(ttsResult.wav_path);
            const audio = new Audio(url);
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
            // 无 wav_path(如 interrupted),不播放
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

  return (
    <div className="panel">
      {/* ===== §8.2 Main Chat:语音输入(W6b-1)===== */}
      <div className="panel-header">§ 8.2 Main Chat · 语音输入</div>
      <h1 className="panel-title">
        Voice <em>input</em> → Skill
      </h1>

      <div className="voice-section">
        <button
          className={`btn voice-button ${listening ? "listening" : ""}`}
          onClick={onVoiceListen}
          disabled={listening || busy}
          aria-pressed={listening}
        >
          <span className="mic-icon" aria-hidden="true">{listening ? "■" : "●"}</span>
          {listening ? "Listening..." : "Start Listening"}
        </button>

        <div className="ptt-status">
          {pttActive && (
            <span className="ptt-active" role="status" aria-live="polite">
              按住 Ctrl+Alt+Space 录音中…
            </span>
          )}
          {ttsPlaying && (
            <button
              type="button"
              aria-label="停止语音反馈"
              onClick={() => {
                // W6b-3b Fix 1:前端 pause() 立即中断播放;同时通知后端置 cancel flag
                // (后端 cancel flag 主要在合成阶段生效,播放阶段由前端控制)。
                if (audioRef.current) {
                  audioRef.current.pause();
                  audioRef.current = null;
                }
                invokeCancelTts().catch(console.error);
                setTtsPlaying(false);
              }}
            >
              停止语音反馈
            </button>
          )}
        </div>

        {listening && (
          <button
            type="button"
            className="voice-cancel-btn"
            onClick={() => cancelVoice().catch(console.error)}
            aria-label="取消录音"
          >
            取消
          </button>
        )}

        {listening && (
          <div className="listening-indicator">
            <span className="dots" aria-hidden="true">
              <span></span>
              <span></span>
              <span></span>
            </span>
            录音中,VAD 检测静音后自动停止
            <div className="partial-text" aria-live="polite">{partialText || "聆听中…"}</div>
          </div>
        )}

        {voiceError && (
          <div className="transcription-display error" role="status" aria-live="polite">
            <div className="label">Error</div>
            <div className="text">{voiceError}</div>
          </div>
        )}

        {voiceResult && (
          <>
            {voiceResult.kind === "success" && (
              <>
                <div className="transcription-display" role="status" aria-live="polite">
                  <div className="label">
                    Transcription {voiceResult.stopped_by_vad ? "(VAD stopped)" : ""}
                  </div>
                  <div className="text">{voiceResult.transcription}</div>
                </div>
                <RouteOutcomeFeedback
                  outcome={voiceResult.route_outcome}
                />
              </>
            )}
            {voiceResult.kind === "no_speech" && (
              <div className="transcription-display no-speech" role="status" aria-live="polite">
                <div className="label">Result</div>
                <div className="text">未检测到语音</div>
              </div>
            )}
            {voiceResult.kind === "timeout" && (
              <>
                <div className="transcription-display" role="status" aria-live="polite">
                  <div className="label">Transcription (timeout)</div>
                  <div className="text">
                    {voiceResult.transcription || "(无转写结果)"}
                  </div>
                </div>
                <RouteOutcomeFeedback
                  outcome={voiceResult.route_outcome}
                />
              </>
            )}
            {voiceResult.kind === "error" && (
              <div className="transcription-display error" role="status" aria-live="polite">
                <div className="label">Error</div>
                <div className="text">{voiceResult.message}</div>
              </div>
            )}
          </>
        )}

        {slots.length > 0 && (
          <div className="chips-container" aria-label="可修改参数">
            {slots.map((slot, idx) => (
              <Chip
                key={`${slot.kind}-${slot.start}-${idx}`}
                slot={slot}
                onClick={(s) => setEditingSlot(s)}
              />
            ))}
          </div>
        )}
        <SlotEditDialog
          slot={editingSlot}
          onSubmit={(slot, newValue) => {
            // 更新本地 slots 列表的 raw 值（transcription 显示不变，规格允许简单替换）
            setSlots((prev) =>
              prev.map((s) =>
                s === slot ? { ...s, raw: newValue } : s,
              ),
            );
            setEditingSlot(null);
          }}
          onClose={() => setEditingSlot(null)}
        />
      </div>

      {/* ===== §5.1 Skill Router(键盘输入 fallback,W6a)===== */}
      <div className="panel-header" style={{ marginTop: 48 }}>
        § 5.1 Skill Router · 文本输入
      </div>
      <h1 className="panel-title">
        Route <em>intent</em> → Skill
      </h1>

      <div className="form-row">
        <label htmlFor="route-text">Text</label>
        <input
          id="route-text"
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="整理下载目录"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onRoute} disabled={busy || listening}>
          Route
        </button>
      </div>

      {routeResult && (
        <div
          className={`route-result ${
            routeResult.kind === "routed" ? "routed" : "unmatched"
          }`}
        >
          {routeResult.kind === "routed" && (
            <>✓ Routed to skill: <strong>{routeResult.skill_id}</strong></>
          )}
          {routeResult.kind === "unmatched" && (
            <>? No skill matched: <strong>{routeResult.text}</strong></>
          )}
          {routeResult.kind === "empty" && <>∅ Empty input</>}
        </div>
      )}

      {/* ===== §5.2 Files Organize(W6a)===== */}
      <div className="panel-header" style={{ marginTop: 48 }}>§ 5.2 Files Organize</div>
      <h1 className="panel-title">
        Run <em>files.organize</em>
      </h1>

      <div className="form-row">
        <label htmlFor="organize-source">Source</label>
        <input
          id="organize-source"
          type="text"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          placeholder="D:/Downloads"
        />
      </div>
      <div className="form-row">
        <label htmlFor="organize-filter">Filter</label>
        <input
          id="organize-filter"
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="*.pdf"
        />
      </div>
      <div className="form-row">
        <label htmlFor="organize-destination">Destination</label>
        <input
          id="organize-destination"
          type="text"
          value={destination}
          onChange={(e) => setDestination(e.target.value)}
          placeholder="D:/Documents/Papers"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onOrganize} disabled={busy || listening}>
          Organize
        </button>
      </div>

      {error && (
        <div className="route-result unmatched" style={{ borderLeftColor: "var(--danger)" }}>
          ⨯ Error: <strong>{error}</strong>
        </div>
      )}

      {organizeResult && (
        <div className="route-result routed">
          <div>
            committed: <strong>{String(organizeResult.committed)}</strong>
          </div>
          <div>
            moved: <strong>{organizeResult.moved_paths.length}</strong> file(s)
          </div>
          <div>
            evidence: <strong>{organizeResult.evidence_strength}</strong>
          </div>
          {organizeResult.compensation_ref && (
            <div>
              compensation_ref: <strong>{organizeResult.compensation_ref}</strong>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Route outcome 反馈组件 —— 显示 Skill 命中 / 未匹配 / 空输入。 */
function RouteOutcomeFeedback({
  outcome,
}: {
  outcome: RouteTextResult;
}) {
  return (
    <div className="route-outcome-feedback">
      <span className="key">Route outcome:</span>
      {outcome.kind === "routed" && (
        <span className="val success">
          ✓ Skill 命中: <strong>{outcome.skill_id}</strong>
        </span>
      )}
      {outcome.kind === "unmatched" && (
        <span className="val warning">
          ? Planner 路径(未命中 Skill): <strong>{outcome.text}</strong>
        </span>
      )}
      {outcome.kind === "empty" && (
        <span className="val">∅ 空输入</span>
      )}
    </div>
  );
}
