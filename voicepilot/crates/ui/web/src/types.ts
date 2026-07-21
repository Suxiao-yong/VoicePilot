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
