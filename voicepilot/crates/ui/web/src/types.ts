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
