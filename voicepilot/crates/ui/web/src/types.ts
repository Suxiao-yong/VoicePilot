export interface EffectManifest {
  sources: FileSnapshot[];
  destination: string;
  conflicts: string[];
  total_bytes: number;
}

export interface FileSnapshot {
  canonical_path: string;
  file_id: string;
  size: number;
  last_write_time: string;
  sha256: string;
}

export type ApprovalDecision = "allow" | "deny" | "modify";

export interface ApprovalRequestPayload {
  approval_request_id: string;
  manifest: EffectManifest;
}

export type RouteTextResult =
  | { kind: "routed"; skill_id: string }
  | { kind: "unmatched"; text: string }
  | { kind: "empty" };

export interface OrganizeInput {
  task_id: string;
  step_id: string;
  source: string;
  filter: string;
  destination: string;
}

export interface OrganizeResult {
  committed: boolean;
  moved_paths: [string, string][];
  evidence_strength: string;
  compensation_ref: string | null;
  error: string | null;
}

export type VoiceListenResult =
  | {
      kind: "success";
      transcription: string;
      route_outcome: RouteTextResult;
      stopped_by_vad: boolean;
    }
  | { kind: "no_speech" }
  | {
      kind: "timeout";
      transcription: string | null;
      route_outcome: RouteTextResult;
    }
  | { kind: "error"; message: string };

export interface TranscriptionFinalPayload {
  transcription: string;
  route_outcome: RouteTextResult;
  stopped_by_vad: boolean;
  slots: Slot[];
}

export interface TranscriptionPartialPayload {
  partial: string;
  timestamp_ms: number;
  slots: Slot[];
}

export interface Settings {
  voice_model_path: string;
  voice_language: string | null;
  voice_threads: number;
  vad_energy_threshold: number;
  vad_max_silence_ms: number;
  vad_min_speech_ms: number;
  voice_max_duration_ms: number;
  voice_chunk_duration_ms: number;
  privacy_mode: boolean;
  compensation_ttl_hours: number;
  // W6c P1 #1:TTS 配置(VP-FR-002 语音反馈),与后端 SettingsDto 对齐
  tts_enabled: boolean;
  tts_model_path: string;
}

export type View = "main" | "settings" | "audit" | "trust" | "skills";

export interface AuditEvent {
  log_id: string;
  task_id: string;
  step_id: string | null;
  event_type: string;
  details: unknown;
  timestamp: string;
  prev_hash: string | null;
  hash: string;
}

export interface McpServer {
  server_id: string;
  name: string;
  version: string;
  transport: string;
  enabled: boolean;
  trusted: boolean;
  protocol_version: string | null;
  allowed_origins: string | null;
  allowed_paths: string | null;
}

export interface Skill {
  skill_id: string;
  version: string;
  enabled: boolean;
  success_count: number;
  avg_latency_ms: number;
  risk_label: string;
}

// ===== W6b-3a Task 4: Diff Preview =====

export type FileKind = "new_file" | "text" | "binary";

export interface DiffResult {
  source_path: string;
  dest_path: string;
  file_kind: FileKind;
  diff_text: string | null;
  truncated: boolean;
  truncate_reason: string | null;
}

// ===== W6b-3a Task 8: ModelDownloadBar =====

export type ModelStatus = "disabled" | "present" | "absent";

export interface DownloadProgressPayload {
  downloaded_bytes: number;
  total_bytes: number | null;
  percent: number | null;
}

// ===== W6b-3b Task 16: Slot + SlotKind (§8.4 Chip 修改) =====

export type SlotKind = "path" | "app" | "number" | "recipient" | "delete_target";

export interface Slot {
  kind: SlotKind;
  raw: string;
  start: number;
  end: number;
  high_risk: boolean;
  // W6c P1 #2:前端状态标记,后端 Rust 不需要(用户修改后置 true,Apply 后清空)
  modified?: boolean;
}
