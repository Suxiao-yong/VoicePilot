import type { Slot, SlotKind } from "../types";

interface ChipProps {
  slot: Slot;
  onClick: (slot: Slot) => void;
  /** 是否低置信(从后端 partial payload 中获取,若置信度低于阈值则加下划线样式)。 */
  lowConfidence?: boolean;
}

const KIND_LABEL: Record<SlotKind, string> = {
  path: "路径",
  app: "应用",
  number: "数量",
  recipient: "收件人",
  delete_target: "删除目标",
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
      title={`${KIND_LABEL[slot.kind]}${slot.high_risk ? "(高风险,需确认)" : ""}${slot.modified ? "(已修改)" : ""}`}
    >
      <span className="chip-kind">{KIND_LABEL[slot.kind]}</span>
      <span className="chip-value">{slot.raw}</span>
      {slot.modified && <span className="chip-modified-mark" aria-hidden="true">✓</span>}
    </button>
  );
}
