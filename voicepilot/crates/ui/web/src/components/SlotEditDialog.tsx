import { useState, useEffect } from "react";
import type { Slot } from "../types";
import { useDialogA11y } from "../useDialogA11y";

interface SlotEditDialogProps {
  slot: Slot | null;
  onSubmit: (slot: Slot, newValue: string) => void;
  onClose: () => void;
}

export function SlotEditDialog({
  slot,
  onSubmit,
  onClose,
}: SlotEditDialogProps) {
  const [value, setValue] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  // 4-8:高风险未勾选的提示从 alert() 改为行内错误文案
  const [confirmError, setConfirmError] = useState(false);
  // 4-8:焦点圈禁 + Esc 统一(不再依赖焦点是否在弹窗内)
  const dialogRef = useDialogA11y(onClose);

  useEffect(() => {
    if (slot) {
      setValue(slot.raw);
      setConfirmed(false);
      setConfirmError(false);
    }
  }, [slot]);

  if (!slot) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    // 高风险 Slot 必须勾选确认（§8.4 不接受纯语音确认）
    if (slot.high_risk && !confirmed) {
      setConfirmError(true);
      return;
    }
    setConfirmError(false);
    onSubmit(slot, value);
  };

  return (
    <div
      ref={dialogRef}
      className="dialog-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="slot-edit-title"
      tabIndex={-1}
    >
      <form className="dialog dialog-sm" onSubmit={handleSubmit}>
        <div className="dialog-head">
          <h3 id="slot-edit-title" className="dialog-title">
            <span className="dot" aria-hidden="true" />
            修改参数
          </h3>
          {slot.high_risk && <span className="badge badge-danger">高风险</span>}
        </div>
        <div className="dialog-body">
          <div className="field">
            <label className="field-label" htmlFor="slot-value">
              值
            </label>
            <input
              id="slot-value"
              className="field-input mono"
              type="text"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              autoFocus
            />
          </div>
          {slot.high_risk && (
            <div>
              <label className="field-check check-gap" htmlFor="slot-confirm">
                <input
                  id="slot-confirm"
                  type="checkbox"
                  checked={confirmed}
                  onChange={(e) => {
                    setConfirmed(e.target.checked);
                    if (e.target.checked) setConfirmError(false);
                  }}
                />
                我已视觉确认此高风险参数（路径/收件人/删除目标）
              </label>
              {confirmError && (
                <p className="field-hint warn" role="alert">
                  高风险参数必须勾选视觉确认
                </p>
              )}
            </div>
          )}
        </div>
        <div className="dialog-foot">
          <button type="button" className="btn" onClick={onClose}>
            取消
          </button>
          <button type="submit" className="btn btn-primary">
            提交
          </button>
        </div>
      </form>
    </div>
  );
}
