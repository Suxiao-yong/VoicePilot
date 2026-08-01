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
  // W7:云端 LLM 配置(OpenAI 兼容,默认 DeepSeek)。
  // llm_enabled=false 或 privacy_mode=true 时,后端 LlmClient::disabled()。
  llm_enabled: boolean;
  llm_api_key: string;
  llm_base_url: string;
  llm_model: string;
  llm_provider_url: string;
  // W7 Plan 4: UIA allowed_apps 白名单(默认 ["notepad", "explorer", "calc"])。
  // 逗号分隔输入,后端 KV 存 JSON 数组字符串。uia feature 关闭时仍可编辑(数据无害)。
  uia_allowed_apps: string[];
}

export type View = "main" | "settings" | "audit" | "trust" | "skills" | "dag-history";

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

// ===== W7 Plan 3: 用户自定义 Skill(从 %APPDATA%\voicepilot\skills\*.md 加载) =====

export interface UserSkill {
  skill_id: string;
  title: string;
  description: string;
  /** 源文件绝对路径(`%APPDATA%\voicepilot\skills\<filename>.md`)。 */
  source_path: string;
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

export type SlotKind =
  | "path"
  | "app"
  | "number"
  | "recipient"
  | "delete_target"
  // W7 新增:LLM 提取的 Slot 类型(regex 不覆盖)
  | "time_range"
  | "url";

export interface Slot {
  kind: SlotKind;
  raw: string;
  start: number;
  end: number;
  high_risk: boolean;
  // W6c P1 #2:前端状态标记,后端 Rust 不需要(用户修改后置 true,Apply 后清空)
  modified?: boolean;
}

// ===== W7: LLM ExtractedSlot =====
// 后端 trust_kernel::llm::types::ExtractedSlot 的前端镜像。
// LLM 路由返回的原始 Slot(open kind 字符串,无字符位置)。
// route_text 将 ExtractedSlot 转换为 Slot 后再返回前端。
export interface ExtractedSlot {
  kind: string;
  raw: string;
  high_risk: boolean;
}

// ===== W8 Plan 5: DAG 相关类型 =====

/** W8 §2.3:DAG 节点(前端镜像,与后端 DagNodeDetailDto 对齐)。 */
export interface DagNode {
  node_id: string;
  skill_id: string;
  risk_ceiling: string; // "E0" | "E1" | "E2" | "E3"
  status: string;
  input_template_json: string;
  output_json: string | null;
  error_message: string | null;
  task_id: string | null;
  step_id: string | null;
  started_at: string | null;
  completed_at: string | null;
}

/** W8 §2.3:DAG 边。 */
export interface DagEdge {
  from: string;
  to: string;
  port_binding: string | null;
}

/** W8 §2.3:DAG 完整详情。 */
export interface DagPlanDetail {
  plan_id: string;
  user_goal: string;
  status: string;
  created_at: string;
  completed_at: string | null;
  max_total_steps: number;
  nodes: DagNode[];
  edges: DagEdge[];
}

/** W8 §2.7:DAG 历史列表项。 */
export interface DagPlanSummary {
  plan_id: string;
  user_goal: string;
  status: string;
  created_at: string;
  completed_at: string | null;
  root_task_id: string | null;
  node_count: number;
  success_rate: number;
}

/** W8 §2.5:task.explain LLM 归因。 */
export interface TaskExplanation {
  explanation_id: string;
  step_id: string;
  root_cause_zh: string;
  category: string;
  suggested_fix: string | null;
  confidence: number;
  llm_model: string | null;
  created_at: string;
}

/** W8 §2.7:DAG 骨架审批请求 payload(后端 emit `dag-approval-request` 事件)。 */
export interface DagApprovalRequestPayload {
  approval_request_id: string;
  plan_id: string;
  user_goal: string;
  max_total_steps: number;
  node_count: number;
  plan_json: DagPlanFull;
}

/** W9 Plan 4:完整 DagPlan(供 Modify 时构造 modified_plan 用)。 */
export interface DagPlanFull {
  plan_id: string;
  user_goal: string;
  nodes: DagNode[];
  edges: DagEdge[];
  loop_specs: Record<string, unknown>;
  max_total_steps: number;
}

/** W8 §2.7:DAG 审批决策。 */
export type DagApprovalDecision = "allow" | "deny" | "modify";

/** W8 §2.7:DAG 状态过滤。 */
export type DagStatusFilter =
  | "all"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled";

/** W8 §2.5:失败工具调用摘要(后端 task.explain 输出的一部分)。 */
export interface FailedToolCall {
  tool_name: string;
  error_message: string;
  timestamp: string | null;
}

/** W8 §2.5:task.explain 完整输出(包含 LLM 归因)。 */
export interface TaskExplainFull {
  step_id: string;
  status: string;
  failed_tool_calls: FailedToolCall[];
  llm_analysis: TaskExplanation | null;
}
