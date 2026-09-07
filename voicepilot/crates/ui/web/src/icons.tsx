/**
 * 内联 SVG 图标集(2026-08-24 4-6)。
 * 全量手写 16×16 stroke 风格,统一 1.75px 圆头;不引图标库(YAGNI)。
 * 用法:<Icon name="mic" className="..." aria-hidden />
 */
const PATHS: Record<string, JSX.Element> = {
  chat: (
    <path d="M2.5 3.5h11a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-2.8 2.4V11.5H2.5a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1Z" />
  ),
  settings: (
    <path d="M8 6a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm0-3.5 1.1 1.4 1.7-.4.9 1.5-1 1.4.6 1.6-1.5 1L9.7 10l-1.7.6L6.5 9.2 5 8.2l.6-1.6-1-1.4.9-1.5 1.7.4L8 2.5Z" />
  ),
  shield: (
    <path d="M8 2.5 3.5 4.25v3.6c0 2.7 1.9 4.6 4.5 5.65 2.6-1.05 4.5-2.95 4.5-5.65v-3.6L8 2.5Zm0 1.35v8" />
  ),
  cube: (
    <path d="M8 2.5 13 5v6l-5 2.5L3 11V5l5-2.5Zm-4.4 2.9L8 7.6l4.4-2.2M8 7.7v6.6" />
  ),
  flow: (
    <path d="M5.5 3.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm5 5a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM5.5 5.5h3.2a2.4 2.4 0 0 1 2.4 2.4v.1M6.5 13.5h2a2 2 0 0 0 2-2v-1" />
  ),
  scroll: (
    <path d="M4 3.5h8a1 1 0 0 1 1 1v1.5H5V9a1.5 1.5 0 0 1-3 0V5a1.5 1.5 0 0 1 1.5-1.5H4Zm-1 3V9a1 1 0 0 0 2 0V6H3Zm2 6h8M3.5 14h9" />
  ),
  mic: (
    <path d="M5.5 3.5a2.5 2.5 0 0 1 5 0v4a2.5 2.5 0 0 1-5 0v-4ZM3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2m0 0H5.5m2.5 0h2.5" />
  ),
  stop: (
    <path d="M5 5.5A.5.5 0 0 1 5.5 5h5a.5.5 0 0 1 .5.5v5a.5.5 0 0 1-.5.5h-5a.5.5 0 0 1-.5-.5v-5Z" />
  ),
  warn: <path d="M8 2.8 13.8 13H2.2L8 2.8Zm0 3.7v3.2m0 1.6v.01" />,
};

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  className,
}: {
  name: IconName;
  className?: string;
}): JSX.Element {
  return (
    <svg
      className={className}
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
