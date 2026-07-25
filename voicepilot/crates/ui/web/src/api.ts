import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  AuditEvent,
  DiffResult,
  McpServer,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
  Skill,
  VoiceListenResult,
  TranscriptionFinalPayload,
  TranscriptionPartialPayload,
  Settings,
  ModelStatus,
  DownloadProgressPayload,
} from "./types";

export async function routeText(text: string): Promise<RouteTextResult> {
  return invoke<RouteTextResult>("route_text_command", { text });
}

export async function organizeFiles(
  input: OrganizeInput
): Promise<OrganizeResult> {
  return invoke<OrganizeResult>("organize_files_command", { input });
}

export async function submitApproval(
  approvalRequestId: string,
  decision: ApprovalDecision
): Promise<boolean> {
  return invoke<boolean>("submit_approval_command", {
    approvalId: approvalRequestId,
    decision,
  });
}

export function onApprovalRequest(
  handler: (payload: ApprovalRequestPayload) => void
): Promise<UnlistenFn> {
  return listen<ApprovalRequestPayload>("approval-request", (event) => {
    handler(event.payload);
  });
}

export async function voiceListen(): Promise<VoiceListenResult> {
  return invoke<VoiceListenResult>("voice_listen_command");
}

export async function cancelVoice(): Promise<void> {
  await invoke("cancel_voice_command");
}

/**
 * Subscribe to `transcription-final` events emitted by the Rust side.
 *
 * TODO(W6b-2): MainView currently uses synchronous `voiceListen()` invoke to
 * get the full result. This listener is reserved for future partial-transcript
 * streaming support (spec issue #47, deferred to W6b-2).
 */
export function onTranscriptionFinal(
  handler: (payload: TranscriptionFinalPayload) => void
): Promise<UnlistenFn> {
  return listen<TranscriptionFinalPayload>("transcription-final", (event) => {
    handler(event.payload);
  });
}

/**
 * Subscribe to `transcription-partial` events(W6b-2 issue #47)。
 * listen 期间每 2s 发射一次,webview 实时显示 partial 转写。
 */
export function onTranscriptionPartial(
  handler: (payload: TranscriptionPartialPayload) => void
): Promise<UnlistenFn> {
  return listen<TranscriptionPartialPayload>("transcription-partial", (e) =>
    handler(e.payload)
  );
}

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings_command");
}

export async function updateSettings(settings: Settings): Promise<void> {
  await invoke("update_settings_command", { settings });
}

export async function listAuditRecent(limit: number): Promise<AuditEvent[]> {
  return invoke<AuditEvent[]>("list_audit_recent_command", { limit });
}

export async function listAuditForTask(taskId: string): Promise<AuditEvent[]> {
  return invoke<AuditEvent[]>("list_audit_for_task_command", { taskId });
}

export async function listMcpServers(): Promise<McpServer[]> {
  return invoke<McpServer[]>("list_mcp_servers_command");
}

export async function toggleMcpServer(serverId: string, enabled: boolean): Promise<void> {
  await invoke("toggle_mcp_server_command", { serverId, enabled });
}

export async function listSkills(): Promise<Skill[]> {
  return invoke<Skill[]>("list_skills_command");
}

export async function toggleSkill(skillId: string, enabled: boolean): Promise<void> {
  await invoke("toggle_skill_command", { skillId, enabled });
}

// ===== W6b-3a Task 4: Diff Preview =====

export async function computeDiff(
  sourcePath: string,
  destPath: string
): Promise<DiffResult> {
  return invoke<DiffResult>("compute_diff_command", {
    sourcePath,
    destPath,
  });
}

// ===== W6b-3a Task 8: ModelDownloadBar =====

export async function isVoiceEnabled(): Promise<boolean> {
  return invoke<boolean>("is_voice_enabled_command");
}

export async function checkModel(): Promise<ModelStatus> {
  return invoke<ModelStatus>("check_model_command");
}

export async function downloadModel(): Promise<string> {
  return invoke<string>("download_model_command");
}

export function onModelDownloadProgress(
  handler: (payload: DownloadProgressPayload) => void
): Promise<UnlistenFn> {
  return listen<DownloadProgressPayload>("model-download-progress", (e) =>
    handler(e.payload)
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
  return await invoke<TtsResult>("tts_command", { text });
}

export async function invokeCancelTts(): Promise<void> {
  await invoke("cancel_tts_command");
}
