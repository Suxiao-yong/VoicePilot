import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
  VoiceListenResult,
  TranscriptionFinalPayload,
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
