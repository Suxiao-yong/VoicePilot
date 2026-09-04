import { useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { Menu, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createDiting, type DitingHandle } from "../pet/ditingScene";
import {
  cancelVoice,
  exitApp,
  onAudioLevel,
  organizeFiles,
  petSetBubbleVisible,
  routeText,
  showMainWindow,
  voiceListenPet,
} from "../api";

/**
 * 桌宠窗口(桌宠化改造)。
 *
 * 窗口 300×360 透明无边框,布局与 Rust 穿透看门狗热区严格对齐
 * (crates/ui/src/pet_commands.rs):
 *   - 兽身谛听渲染区:底部中央 (45,135)-(255,360)
 *   - 确认气泡:顶部居中 (10,5)-(290,135)(仅 confirming 态渲染并同步可见性)
 *
 * 交互:
 *   - 左键单击(位移 ≤6px)= 切换录音;>6px = 原生拖动窗口
 *   - 录音为手动停止模式(voice_listen_command manualStop=true),
 *     再次单击走 kill_switch,样本不丢,返回 Timeout{transcription}
 *   - confirming 气泡:✓执行(files.organize 且槽位齐 → 直接整理,高风险自动
 *     弹主界面审批)/ ✕放弃 / 点文字跳转主界面改字(pet-edit-text 事件)
 *   - 右键菜单:打开主界面 / 开始录音 / 退出程序(唯一退出入口)
 */

type Phase = "idle" | "recording" | "confirming" | "executing";

/** 诊断探针(main.tsx 注入;桌宠白屏/无反应排查) */
function probe(msg: string): void {
  try {
    // SAFETY: __probe is an optional main.tsx-injected diagnostic hook;
    // absent in production builds, and the `?.` call is a no-op then.
    (window as unknown as { __probe?: (m: string) => void }).__probe?.(
      `[pet] ${msg}`,
    );
  } catch {
    /* ignore */
  }
}

export function PetWindow() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const ditingRef = useRef<DitingHandle | null>(null);

  const [phase, setPhase] = useState<Phase>("idle");
  const [transcription, setTranscription] = useState("");
  /** Chat 兜底回答:直接展示,不进执行确认流 */
  const [chatAnswer, setChatAnswer] = useState<string | null>(null);
  /** 非阻塞提示(未检测到语音等):小气泡,不可点,不同步给看门狗 */
  const [hint, setHint] = useState<string | null>(null);

  // 挂载探针(挪进 effect 避免每次 render 都打日志)
  useEffect(() => {
    probe("component-mount");
  }, []);

  // 记录录音 promise 是否已被取消路径处理,避免 setState 于卸载后
  const aliveRef = useRef(true);
  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  // ===== 谛听场景初始化(StrictMode 安全:create→dispose→create) =====
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      probe("mount-no-canvas");
      return;
    }
    let handle: DitingHandle | null = null;
    try {
      handle = createDiting(canvas);
      ditingRef.current = handle;
      probe("diting-created");
      // 首帧探针:rAF 不跑 = 渲染循环/合成器异常
      requestAnimationFrame(() => probe("first-raf"));
    } catch (e) {
      console.error("[pet] createDiting failed:", e);
      probe(`diting-failed ${e instanceof Error ? e.message : String(e)}`);
    }
    return () => {
      handle?.dispose();
      ditingRef.current = null;
    };
  }, []);

  // ===== 状态机 → 动画映射:idle 英气站姿 / recording 御风 /
  //       confirming 挺胸 / executing 抖鬃 =====
  useEffect(() => {
    ditingRef.current?.setAnim(
      phase === "recording"
        ? "wind"
        : phase === "confirming"
          ? "proud"
          : phase === "executing"
            ? "shake"
            : "idle",
    );
  }, [phase]);

  // ===== audio-level(RmsEmitterRecorder 广播)→ 鬃毛摆幅 =====
  useEffect(() => {
    const unlisten = onAudioLevel((level) =>
      ditingRef.current?.setLevel(level),
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // ===== 气泡可见性同步给穿透看门狗 =====
  const bubbleVisible = phase === "confirming";
  useEffect(() => {
    petSetBubbleVisible(bubbleVisible).catch(() => {});
  }, [bubbleVisible]);
  useEffect(() => {
    // 卸载兜底:气泡热区必须复位,否则透明区永久可交互
    return () => {
      petSetBubbleVisible(false).catch(() => {});
    };
  }, []);

  // ===== 提示 toast =====
  const hintTimer = useRef<number | null>(null);
  function flashHint(msg: string): void {
    setHint(msg);
    if (hintTimer.current !== null) window.clearTimeout(hintTimer.current);
    hintTimer.current = window.setTimeout(() => setHint(null), 2600);
  }

  // ===== 录音控制 =====
  async function startRecording(): Promise<void> {
    setHint(null);
    setPhase("recording");
    probe("listen-begin");
    try {
      const r = await voiceListenPet();
      probe(`listen-result ${r.kind}`);
      if (!aliveRef.current) return;
      if (
        (r.kind === "success" || r.kind === "timeout") &&
        r.transcription?.trim()
      ) {
        // Chat 兜底:直接展示回答,不进执行确认流
        if (r.route_outcome?.kind === "chat" && r.route_outcome.text.trim()) {
          setChatAnswer(r.route_outcome.text);
          setTranscription("");
        } else {
          setChatAnswer(null);
          setTranscription(r.transcription.trim());
        }
        // 3-4 气泡穿透竞态:必须先让 Rust 看门狗把 BUBBLE_RECT 纳入交互热区
        // (bubble_visible 原子量置位 + 下一 100ms tick 生效),再渲染气泡,
        // 否则气泡出现后头 100-200ms 点 ✓/✕ 会穿透到桌面。
        await petSetBubbleVisible(true).catch(() => {});
        setPhase("confirming");
      } else if (r.kind === "no_speech") {
        setPhase("idle");
        flashHint("没听到声音哦");
      } else {
        // timeout 无转写 / error
        const msg = r.kind === "error" ? r.message : "没听清,再试一次?";
        setPhase("idle");
        flashHint(msg.slice(0, 40));
      }
    } catch (e) {
      if (!aliveRef.current) return;
      setPhase("idle");
      flashHint((e instanceof Error ? e.message : String(e)).slice(0, 40));
    }
  }

  async function stopRecording(): Promise<void> {
    // kill_switch:listen_with_cancel break → vad.detect(buffer) →
    // Timeout{samples} 带转写,startRecording 的 await 会拿到结果
    await cancelVoice().catch(() => {});
  }

  function toggleRecord(): void {
    probe(`toggle-record phase=${phase}`);
    if (phase === "idle") void startRecording();
    else if (phase === "recording") void stopRecording();
  }

  // ===== 拖拽 vs 单击(>6px 位移交给系统 startDragging) =====
  const dragStart = useRef<{ x: number; y: number; moved: boolean } | null>(
    null,
  );
  function onMouseDown(e: React.MouseEvent): void {
    if (e.button !== 0) return;
    dragStart.current = { x: e.clientX, y: e.clientY, moved: false };
  }
  function onMouseMove(e: React.MouseEvent): void {
    const d = dragStart.current;
    if (!d || d.moved) return;
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > 6) {
      d.moved = true;
      getCurrentWindow()
        .startDragging()
        .catch(() => {});
    }
  }
  function onMouseUp(): void {
    const d = dragStart.current;
    dragStart.current = null;
    if (!d || d.moved) return; // 拖拽结束的 mouseup 被系统消费,不会到这里
    toggleRecord();
  }

  // ===== 气泡动作 =====
  async function editInMain(text: string): Promise<void> {
    await emit("pet-edit-text", text).catch(() => {});
    await showMainWindow().catch(() => {});
    setChatAnswer(null);
    setPhase("idle");
    setTranscription("");
  }

  async function onConfirm(): Promise<void> {
    const text = transcription;
    if (!text) return;
    setPhase("executing");
    try {
      const route = await routeText(text);
      if (
        route.kind === "routed" &&
        route.skill_id.includes("organize") &&
        route.slots.filter((s) => s.kind === "path").length >= 2
      ) {
        const paths = route.slots
          .filter((s) => s.kind === "path")
          .map((s) => s.raw);
        // 高风险操作:organizeFiles 内部触发 Trust Kernel 审批链,
        // approver.rs 已在 emit 前唤醒隐藏的主界面
        const res = await organizeFiles({
          task_id: `pet-${Date.now()}`,
          step_id: `s-${Date.now()}`,
          source: paths[0],
          filter: "*",
          destination: paths[paths.length - 1],
        });
        if (!aliveRef.current) return;
        flashHint(
          res.committed
            ? `已整理 ${res.moved_paths.length} 个文件`
            : "整理被拒绝或失败",
        );
      } else {
        // 槽位不全 / 非 files.organize / Unmatched → 主界面完整交互
        if (!aliveRef.current) return;
        await editInMain(text);
        return;
      }
    } catch (e) {
      console.error("[pet] execute failed:", e);
      flashHint("执行失败,去主界面看看?");
    } finally {
      if (aliveRef.current) {
        setPhase("idle");
        setTranscription("");
      }
    }
  }

  function onDiscard(): void {
    setChatAnswer(null);
    setTranscription("");
    setPhase("idle");
  }

  /** Chat 回答确认:仅关闭气泡,不执行、不跳转 */
  function dismissChat(): void {
    setChatAnswer(null);
    setTranscription("");
    setPhase("idle");
  }

  // ===== 右键菜单(原生 OS 菜单,借鉴 BongoCat)=====
  // 自绘 HTML 菜单在穿透看门狗架构下有个致命坑:菜单渲染在兽身热区外,
  // 光标一移过去看门狗就把窗口切成点击穿透,所有菜单点击全部落空。
  // 原生 Menu.popup 是独立 OS 窗口,不依赖本窗口的交互态,天然无此问题。
  async function openNativeMenu(): Promise<void> {
    const win = getCurrentWindow();
    probe("context-menu-open");
    // Windows 下置顶窗口会盖住原生菜单:弹菜单前临时取消置顶,关掉后恢复
    const wasTop = await win.isAlwaysOnTop().catch(() => true);
    if (wasTop) await win.setAlwaysOnTop(false).catch(() => {});
    try {
      const menu = await Menu.new({
        items: [
          {
            id: "open-main",
            text: "打开主界面",
            action: () => void showMainWindow().catch(() => {}),
          },
          {
            id: "toggle-rec",
            text: phase === "recording" ? "停止录音" : "开始录音",
            enabled: phase !== "executing",
            action: () => toggleRecord(),
          },
          await PredefinedMenuItem.new({ item: "Separator" }),
          {
            id: "exit-app",
            text: "退出程序",
            action: () => void exitApp().catch(() => {}),
          },
        ],
      });
      await menu.popup();
    } finally {
      if (wasTop) await win.setAlwaysOnTop(true).catch(() => {});
    }
  }

  return (
    <div
      className="pet-root"
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onContextMenu={(e) => {
        e.preventDefault();
        void openNativeMenu();
      }}
    >
      {/* 兽身渲染区:与看门狗 BEAST_RECT (45,135)-(255,360) 对齐 */}
      <canvas ref={canvasRef} className="pet-canvas" />

      {/* 确认气泡:与看门狗 BUBBLE_RECT (10,5)-(290,135) 对齐 */}
      {phase === "confirming" && (
        <div className="pet-bubble">
          <div
            className="pet-bubble-text"
            title="点击到主界面修改文字"
            onClick={() => void editInMain(chatAnswer ?? transcription)}
          >
            {chatAnswer ?? transcription}
          </div>
          <div className="pet-bubble-actions">
            <button
              type="button"
              className="pet-btn pet-btn-ok"
              disabled={!(chatAnswer ?? transcription).trim()}
              onClick={() => (chatAnswer ? dismissChat() : void onConfirm())}
            >
              ✓ {chatAnswer ? "知道了" : "执行"}
            </button>
            <button
              type="button"
              className="pet-btn pet-btn-no"
              onClick={onDiscard}
            >
              ✕ 放弃
            </button>
          </div>
        </div>
      )}

      {/* 非交互提示(不可点,不占热区) */}
      {hint && (
        <div className="pet-hint" role="status">
          {hint}
        </div>
      )}

      {/* 录音状态指示(纯视觉,不挡热区外点击) */}
      {phase === "recording" && (
        <div className="pet-rec-dot" aria-hidden="true" />
      )}
    </div>
  );
}
