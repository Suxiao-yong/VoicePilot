import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
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
