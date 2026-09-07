import { useEffect, useRef } from "react";

/**
 * 弹窗可访问性最小 hook(2026-08-24 4-8)。
 * - 打开时聚焦容器(初始焦点交给 caller 的 focusRef,缺省聚焦首个可聚焦元素)
 * - Tab 循环圈禁:焦点不逃出弹窗(aria-modal 的配套行为)
 * - Esc 统一走 window listener 回调(不依赖焦点是否在弹窗内)
 * 15 行内实现,不引第三方 focus-trap 库(YAGNI)。
 */
export function useDialogA11y(
  onEscape: () => void,
  opts?: { focusRef?: React.RefObject<HTMLElement | null>; open?: boolean },
): React.RefObject<HTMLDivElement> {
  const ref = useRef<HTMLDivElement>(null);
  const open = opts?.open ?? true;
  const focusRef = opts?.focusRef ?? null;

  useEffect(() => {
    if (!open) return;
    const root = ref.current;
    if (!root) return;

    // 初始焦点:caller 指定优先,否则弹窗内第一个可聚焦元素
    if (focusRef?.current) {
      focusRef.current.focus();
    } else {
      const first = root.querySelector<HTMLElement>(
        "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
      );
      first?.focus();
    }

    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.preventDefault();
        onEscape();
        return;
      }
      if (e.key !== "Tab") return;
      // Tab 圈禁:首尾元素循环
      const focusables = Array.from(
        root.querySelectorAll<HTMLElement>(
          "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])",
        ),
      ).filter((el) => !el.hasAttribute("disabled"));
      if (focusables.length === 0) return;
      // prettier-ignore: es2021 lib 不支持 Array.at,索引访问与目标一致
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || active === root)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onEscape, focusRef]);

  return ref;
}
