import type { Slot, SlotKind } from "../types";

interface ChipProps {
  slot: Slot;
  onClick: (slot: Slot) => void;
  /** 低置信（从后端 partial payload 获取，低于阈值加下划线样式）。 */
  lowConfidence?: boolean;
}

const KIND_LABEL: Record<SlotKind, string> = {
  path: "路径",
  app: "应用",
  number: "数量",
  recipient: "收件人",
  delete_target: "删除目标",
  time_range: "时间范围",
  url: "URL",
  // 2026 原子快路由扩展：kind 即后端 input 名，直显英文名即可。
  action: "action",
  app_name: "app_name",
  audio_only: "audio_only",
  body: "body",
  command: "command",
  content: "content",
  days: "days",
  days_ahead: "days_ahead",
  destination: "destination",
  direction: "direction",
  end: "end",
  filter: "filter",
  format: "format",
  keys: "keys",
  label: "label",
  limit: "limit",
  operation: "operation",
  query: "query",
  save_path: "save_path",
  seconds: "seconds",
  source: "source",
  source_filter: "source_filter",
  start: "start",
  subject: "subject",
  target: "target",
  target_step_id: "target_step_id",
  target_task_id: "target_task_id",
  text: "text",
  time: "time",
  title: "title",
  to: "to",
};

export function Chip({ slot, onClick, lowConfidence }: ChipProps) {
  const className = [
    "chip",
    `chip-${slot.kind}`,
    slot.high_risk ? "chip-high-risk" : "",
    lowConfidence ? "chip-low-confidence" : "",
    slot.modified ? "chip-modified" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type="button"
      className={className}
      onClick={() => onClick(slot)}
      title={`${KIND_LABEL[slot.kind]}${slot.high_risk ? "（高风险，需确认）" : ""}${slot.modified ? "（已修改）" : ""}`}
    >
      <span className="chip-kind">{KIND_LABEL[slot.kind]}</span>
      <span className="chip-value">{slot.raw}</span>
      {slot.modified && (
        <span className="chip-modified-mark" aria-hidden="true">
          ✓
        </span>
      )}
    </button>
  );
}
