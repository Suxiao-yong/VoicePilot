import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  AuditEvent,
  DagApprovalDecision,
  DagApprovalRequestPayload,
  DagPlanDetail,
  DagPlanFull,
  DagPlanSummary,
  DagStatusFilter,
  DiffResult,
  McpServer,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
  Skill,
  TaskExplanation,
  UserSkill,
  VoiceListenResult,
  TranscriptionFinalPayload,
  TranscriptionPartialPayload,
  Settings,
  SettingsUpdate,
  LlmTestInput,
  LlmTestResult,
  ModelStatus,
  DownloadProgressPayload,
  ImportMcpResult,
} from "./types";

// ponytail: preview-safe guard — plain http://localhost:4173 has no Tauri internals,
// raw `invoke` would throw `Cannot read properties of undefined (reading 'invoke')`.
// 2026-08-24 修复(2-4):用一次性 `__TAURI_INTERNALS__` 探测替代纯字符串匹配错误分类——
// 避免真实后端错误文案碰巧含 "invoke"/"undefined" 时被误判成 preview 并返回假 mock 值。
const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriInvoke<T>(cmd: string, args?: unknown): Promise<T> {
  // 必须先调 invoke:vitest 单测 mock 了 @tauri-apps/api/core 的 invoke 并断言其被调用
  try {
    // SAFETY: args 是命令入参对象或 undefined;Record<string, unknown> 与 invoke 签名兼容,命令各自在后端校验
    return await invoke<T>(cmd, args as unknown as Record<string, unknown>);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    const isMissingTauri =
      !IS_TAURI || msg.includes("__TAURI") || msg.includes("reading 'invoke'");
    if (isMissingTauri) {
      console.warn(`[preview] invoke ${cmd} mocked — not in Tauri`);
      // preview 各命令 mock 形状的命名联合(边界具名,后续只做一次收敛 cast)
      type PreviewMock =
        | boolean
        | string
        | { llm_api_key_present: boolean }
        | unknown[]
        | null
        | undefined; // default 分支:未注册命令在 preview 下不可用
      // SAFETY: switch case 里注明每个命令的 mock 形状与名义返回类型一一对应
      const mock = ((): PreviewMock => {
        switch (cmd) {
          case "is_voice_enabled_command":
            return true; // -> boolean
          case "check_model_command":
            return "ready"; // -> ModelStatus
          case "get_settings_command":
            return { llm_api_key_present: false }; // -> Settings 非 secret 子集
          case "list_audit_recent_command":
          case "list_audit_for_task_command":
          case "list_mcp_servers_command":
          case "list_skills_command":
          case "list_user_skills_command":
          case "reload_skills_command":
          case "list_dag_history_command":
            return []; // -> 列表类型
          case "get_dag_plan_command":
          case "get_task_explanation_command":
            return null; // -> 可空详情
          default:
            return undefined;
        }
      })();
      if (mock === undefined) {
        return Promise.reject(
          new Error(`[preview] ${cmd} unavailable outside Tauri`),
        );
      }
      // SAFETY: 上面的 case 注释逐命令标注了形状;invoke 泛型 T 无法在不 cast 时收窄联合
      return mock as unknown as T;
    }
    throw e;
  }
}

function tauriListen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  // 明确非 Tauri 环境(preview / 单测):直接给 no-op,不再调用 listen
  if (!IS_TAURI) return Promise.resolve(() => {});
  try {
    // SAFETY: 事件 payload 由监听器泛型 T 声明,cast 只做 TS 侧形状收窄
    const p = listen<T>(event, (e) => handler(e as unknown as { payload: T }));
    // listen may reject if Tauri internals missing
    return p.catch((e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("__TAURI") || msg.includes("invoke")) {
        console.warn(`[preview] listen ${event} mocked — not in Tauri`);
        return () => {};
      }
      throw e;
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("__TAURI") || msg.includes("invoke")) {
      console.warn(`[preview] listen ${event} mocked — not in Tauri`);
      return Promise.resolve(() => {});
    }
    throw e;
  }
}

export async function routeText(text: string): Promise<RouteTextResult> {
  return tauriInvoke<RouteTextResult>("route_text_command", { text });
}

export async function organizeFiles(
  input: OrganizeInput,
): Promise<OrganizeResult> {
  return tauriInvoke<OrganizeResult>("organize_files_command", { input });
}

export async function submitApproval(
  approvalRequestId: string,
  decision: ApprovalDecision,
): Promise<boolean> {
  return tauriInvoke<boolean>("submit_approval_command", {
    approvalId: approvalRequestId,
    decision,
  });
}

export function onApprovalRequest(
  handler: (payload: ApprovalRequestPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen<ApprovalRequestPayload>("approval-request", (event) => {
    handler(event.payload);
  });
}

export async function voiceListen(): Promise<VoiceListenResult> {
  return tauriInvoke<VoiceListenResult>("voice_listen_command");
}

export async function cancelVoice(): Promise<void> {
  await tauriInvoke("cancel_voice_command");
}

/**
 * Subscribe to `transcription-final` events emitted by the Rust side.
 *
 * TODO(W6b-2): MainView currently uses synchronous `voiceListen()` invoke to
 * get the full result. This listener is reserved for future partial-transcript
 * streaming support (spec issue #47, deferred to W6b-2).
 */
export function onTranscriptionFinal(
  handler: (payload: TranscriptionFinalPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen<TranscriptionFinalPayload>(
    "transcription-final",
    (event) => {
      handler(event.payload);
    },
  );
}

/**
 * Subscribe to `transcription-partial` events(W6b-2 issue #47)。
 * listen 期间每 2s 发射一次,webview 实时显示 partial 转写。
 */
export function onTranscriptionPartial(
  handler: (payload: TranscriptionPartialPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen<TranscriptionPartialPayload>(
    "transcription-partial",
    (e) => handler(e.payload),
  );
}

export async function getSettings(): Promise<Settings> {
  return tauriInvoke<Settings>("get_settings_command");
}

export async function updateSettings(settings: SettingsUpdate): Promise<void> {
  await tauriInvoke("update_settings_command", { settings });
}

/** 测试云端 LLM 连接：只读探针，不持久化任何东西（保存前可测）。 */
export async function testLlm(input: LlmTestInput): Promise<LlmTestResult> {
  return tauriInvoke<LlmTestResult>("test_llm_command", { input });
}

export async function listAuditRecent(limit: number): Promise<AuditEvent[]> {
  return tauriInvoke<AuditEvent[]>("list_audit_recent_command", { limit });
}

export async function listAuditForTask(taskId: string): Promise<AuditEvent[]> {
  return tauriInvoke<AuditEvent[]>("list_audit_for_task_command", { taskId });
}

export async function listMcpServers(): Promise<McpServer[]> {
  return tauriInvoke<McpServer[]>("list_mcp_servers_command");
}

export async function toggleMcpServer(
  serverId: string,
  enabled: boolean,
): Promise<void> {
  await tauriInvoke("toggle_mcp_server_command", { serverId, enabled });
}

// ===== Wave 2 Task 2.2: MCP Plugin 配置入口 =====

/** 注册一个新的 MCP Plugin(校验在后端 register_mcp_server_command 完成)。 */
export async function registerMcpServer(server: McpServer): Promise<void> {
  await tauriInvoke("register_mcp_server_command", { server });
}

/** 移除一个 MCP Plugin;仍被 User Skill 引用的 server 会被后端拒绝。 */
export async function removeMcpServer(serverId: string): Promise<void> {
  await tauriInvoke("remove_mcp_server_command", { serverId });
}

export async function listSkills(): Promise<Skill[]> {
  return tauriInvoke<Skill[]>("list_skills_command");
}

export async function toggleSkill(
  skillId: string,
  enabled: boolean,
): Promise<void> {
  await tauriInvoke("toggle_skill_command", { skillId, enabled });
}

// ===== W6b-3a Task 4: Diff Preview =====

export async function computeDiff(
  sourcePath: string,
  destPath: string,
): Promise<DiffResult> {
  return tauriInvoke<DiffResult>("compute_diff_command", {
    sourcePath,
    destPath,
  });
}

// ===== W6b-3a Task 8: ModelDownloadBar =====

export async function isVoiceEnabled(): Promise<boolean> {
  return tauriInvoke<boolean>("is_voice_enabled_command");
}

export async function checkModel(): Promise<ModelStatus> {
  return tauriInvoke<ModelStatus>("check_model_command");
}

export async function downloadModel(): Promise<string> {
  return tauriInvoke<string>("download_model_command");
}

export function onModelDownloadProgress(
  handler: (payload: DownloadProgressPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen<DownloadProgressPayload>("model-download-progress", (e) =>
    handler(e.payload),
  );
}

// ===== W6b-3b Task 13: TTS 播放 / 打断 =====

/**
 * TTS 播放结果(W6b-3b Fix 1:`wav_path` 返回合成的 WAV 文件绝对路径,
 * 由前端 `<audio>` 元素通过 `convertFileSrc` 播放)。
 */
export interface TtsResult {
  played: boolean;
  interrupted: boolean;
  sample_count: number;
  wav_path: string | null;
  error: string | null;
}

export async function invokeTts(text: string): Promise<TtsResult> {
  return await tauriInvoke<TtsResult>("tts_command", { text });
}

export async function invokeCancelTts(): Promise<void> {
  await tauriInvoke("cancel_tts_command");
}

// ===== W7 Plan 3: 用户自定义 Skill 导入 / 列表 / 重载 =====

/** 标准 SKILL.md 目录式导入。 */
export async function invokeImportSkill(
  sourcePath: string,
): Promise<UserSkill> {
  return tauriInvoke<UserSkill>("import_skill_command", { sourcePath });
}

// ===== 主流标准 MCP JSON 兼容(2026-08-24 统一) =====

/** 导入标准 mcpServers JSON(Claude/Cursor/Windsurf 的 `mcpServers` 或 VS Code 的 `servers` 根键)。 */
export async function importMcpServers(json: string): Promise<ImportMcpResult> {
  return tauriInvoke<ImportMcpResult>("import_mcp_servers_command", { json });
}

/** 导出标准 mcpServers JSON(剔除 trusted/allowed_paths 等私有字段,可在其它客户端复用)。 */
export async function exportMcpServers(): Promise<string> {
  return tauriInvoke<string>("export_mcp_servers_command");
}

/** 重扫 skills 目录(标准 {name}/SKILL.md),upsert 到 DB,返回当前列表。 */
export async function invokeReloadSkills(): Promise<UserSkill[]> {
  return tauriInvoke<UserSkill[]>("reload_skills_command");
}

/** 列出当前用户自定义 Skill(重新扫描 skills 目录)。 */
export async function invokeListUserSkills(): Promise<UserSkill[]> {
  return tauriInvoke<UserSkill[]>("list_user_skills_command");
}

// ===== W8 Plan 5: DAG 相关 API =====

/** 提交 DAG 骨架审批决策。 */
export async function approveDagSkeleton(
  approvalRequestId: string,
  decision: DagApprovalDecision,
  modifiedPlan?: DagPlanFull,
): Promise<boolean> {
  return tauriInvoke<boolean>("approve_dag_skeleton_command", {
    approvalRequestId,
    decision,
    modifiedPlan,
  });
}

/** 监听 `dag-approval-request` 事件(后端 TauriApprover::approve_dag_skeleton emit)。 */
export function onDagApprovalRequest(
  handler: (payload: DagApprovalRequestPayload) => void,
): Promise<UnlistenFn> {
  return tauriListen<DagApprovalRequestPayload>(
    "dag-approval-request",
    (event) => {
      handler(event.payload);
    },
  );
}

/** 分页 + 状态过滤查询 DAG 历史。 */
export async function listDagHistory(
  limit: number = 20,
  offset: number = 0,
  filter: DagStatusFilter = "all",
): Promise<DagPlanSummary[]> {
  return tauriInvoke<DagPlanSummary[]>("list_dag_history_command", {
    limit,
    offset,
    filter,
  });
}

/** 查询单个 DAG 完整详情。 */
export async function getDagPlan(
  planId: string,
): Promise<DagPlanDetail | null> {
  return tauriInvoke<DagPlanDetail | null>("get_dag_plan_command", { planId });
}

/** 查询 step 的 LLM 失败归因。 */
export async function getTaskExplanation(
  stepId: string,
): Promise<TaskExplanation | null> {
  return tauriInvoke<TaskExplanation | null>("get_task_explanation_command", {
    stepId,
  });
}

// ===== 桌宠化改造:双窗口架构 / 手动停止录音 =====

/**
 * 桌宠单击录音(手动停止模式):禁用 VAD 自动停(max_silence=30s),
 * 再次单击调 `cancelVoice()` 结束,返回 Timeout{transcription} 不丢样本。
 */
export async function voiceListenPet(): Promise<VoiceListenResult> {
  return tauriInvoke<VoiceListenResult>("voice_listen_command", {
    manualStop: true,
  });
}

/** 显示并聚焦主窗口(桌宠右键菜单 / 气泡"去主界面")。 */
export async function showMainWindow(): Promise<void> {
  await tauriInvoke("show_main_window");
}

/** 隐藏主窗口到后台(进程随桌宠常驻)。 */
export async function hideMainWindow(): Promise<void> {
  await tauriInvoke("hide_main_window");
}

/** 退出应用 —— 唯一退出路径(桌宠右键菜单);关主界面只是隐藏。 */
export async function exitApp(): Promise<void> {
  await tauriInvoke("exit_app");
}

/** 桌宠确认气泡可见性同步给后端穿透看门狗(热区判定)。 */
export async function petSetBubbleVisible(visible: boolean): Promise<void> {
  await tauriInvoke("pet_set_bubble_visible", { visible });
}

/** 订阅 `audio-level`(归一化 RMS 电平 0..=1),驱动桌宠动画。 */
export function onAudioLevel(
  handler: (level: number) => void,
): Promise<UnlistenFn> {
  return tauriListen<{ level: number }>("audio-level", (e) =>
    handler(e.payload.level),
  );
}

/**
 * 监听桌宠"跳转改字"事件(pet → main 广播):
 * PetWindow 气泡点文字时 emit,MainView 把文字填入 composer 并聚焦。
 */
export function onPetEditText(
  handler: (text: string) => void,
): Promise<UnlistenFn> {
  return tauriListen<string>("pet-edit-text", (e) => handler(e.payload));
}
