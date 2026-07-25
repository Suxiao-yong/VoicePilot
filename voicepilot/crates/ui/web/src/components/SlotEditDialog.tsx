import { useState, useEffect } from "react";
import type { Slot } from "../types";

interface SlotEditDialogProps {
  slot: Slot | null;
  onSubmit: (slot: Slot, newValue: string) => void;
  onClose: () => void;
}

export function SlotEditDialog({ slot, onSubmit, onClose }: SlotEditDialogProps) {
  const [value, setValue] = useState("");
  const [confirmed, setConfirmed] = useState(false);

  useEffect(() => {
    if (slot) {
      setValue(slot.raw);
      setConfirmed(false);
    }
  }, [slot]);

  if (!slot) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    // 高风险 Slot 必须勾选确认(§8.4 不接受纯语音确认)
    if (slot.high_risk && !confirmed) {
      alert("高风险参数必须勾选视觉确认");
      return;
    }
    onSubmit(slot, value);
  };

  const handleEsc = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    }
  };

  return (
    <div
      className="slot-edit-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="slot-edit-title"
      onKeyDown={handleEsc}
      tabIndex={-1}
    >
      <form className="slot-edit-dialog" onSubmit={handleSubmit}>
        <h3 id="slot-edit-title">修改参数</h3>
        <label htmlFor="slot-value">值</label>
        <input
          id="slot-value"
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          autoFocus
        />
        {slot.high_risk && (
          <div className="slot-edit-confirm">
            <label htmlFor="slot-confirm">
              <input
                id="slot-confirm"
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
              />
              我已视觉确认此高风险参数(路径/收件人/删除目标)
            </label>
          </div>
        )}
        <div className="slot-edit-buttons">
          <button type="submit">提交</button>
          <button type="button" onClick={onClose}>
            取消
          </button>
        </div>
      </form>
    </div>
  );
}
