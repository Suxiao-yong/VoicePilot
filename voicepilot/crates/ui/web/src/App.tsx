import { useEffect, useState } from "react";
import {
  onApprovalRequest,
  onClarificationRequest,
  onDagApprovalRequest,
} from "./api";
import type {
  ApprovalRequestPayload,
  ClarificationRequestPayload,
  DagApprovalRequestPayload,
  View,
} from "./types";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { ClarificationDialog } from "./components/ClarificationDialog";
import { SettingsView } from "./components/SettingsView";
import { AuditViewerView } from "./components/AuditViewerView";
import { TrustCenterView } from "./components/TrustCenterView";
import { SkillsManagerView } from "./components/SkillsManagerView";
import { DagHistoryView } from "./components/DagHistoryView";
import { DagApprovalDialog } from "./components/DagApprovalDialog";
import { KillSwitchBar } from "./components/KillSwitchBar";
import { ModelDownloadBar } from "./components/ModelDownloadBar";
import { Icon, type IconName } from "./icons";

// 4-6:全量内联 SVG 图标(icons.tsx),导航不再是纯文字
const NAV_ITEMS: { view: View; label: string; icon: IconName }[] = [
  { view: "main", label: "Main Chat", icon: "chat" },
  { view: "settings", label: "Settings", icon: "settings" },
  { view: "audit", label: "Audit Viewer", icon: "scroll" },
  { view: "trust", label: "Trust Center", icon: "shield" },
  { view: "skills", label: "Skills Manager", icon: "cube" },
  { view: "dag-history", label: "DAG History", icon: "flow" },
];

const NARROW_BREAKPOINT = 768;

type ApprovalQueueItem =
  | { kind: "approval"; payload: ApprovalRequestPayload }
  | { kind: "dag"; payload: DagApprovalRequestPayload }
  | { kind: "clarify"; payload: ClarificationRequestPayload };

export function App(): JSX.Element {
  const [view, setView] = useState<View>("main");
  // 审批队列(2026-08-24 修复连坐):approval + DAG 审批可同时到达,
  // 旧实现双 modal 叠渲染,一次 Esc 连拒所有请求。改为先进先出队列,
  // 只渲染队首一个,Esc/dismiss 只弹队首(deny 语义不变)。
  const [queue, setQueue] = useState<ApprovalQueueItem[]>([]);
  // 响应式布局(W6a Fast-Follow):窄窗口隐藏 sidebar,改为 overlay
  const [isNarrow, setIsNarrow] = useState<boolean>(
    window.innerWidth < NARROW_BREAKPOINT,
  );
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(false);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) =>
      setQueue((q) => [...q, { kind: "approval", payload }]),
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // W8 Plan 5: 监听 DAG 骨架审批请求(后端 TauriApprover::approve_dag_skeleton emit)
  useEffect(() => {
    const unlisten = onDagApprovalRequest((payload) =>
      setQueue((q) => [...q, { kind: "dag", payload }]),
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 追问卡：与审批同队列（先进先出，一次只弹队首）。
  useEffect(() => {
    const unlisten = onClarificationRequest((payload) =>
      setQueue((q) => [...q, { kind: "clarify", payload }]),
    );
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 监听窗口尺寸变化(W6a Fast-Follow 响应式)
  useEffect(() => {
    const onResize = () => {
      setIsNarrow(window.innerWidth < NARROW_BREAKPOINT);
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
    };
  }, []);

  // 从窄变宽时自动关闭 overlay(避免宽屏残留 open state)
  useEffect(() => {
    if (!isNarrow) {
      setSidebarOpen(false);
    }
  }, [isNarrow]);

  useEffect(() => {
    const onNavigate = (e: Event): void => {
      const detail = (e as CustomEvent<string>).detail;
      if (detail === "main") setView("main");
    };
    window.addEventListener("voicepilot:navigate", onNavigate as EventListener);
    return () =>
      window.removeEventListener(
        "voicepilot:navigate",
        onNavigate as EventListener,
      );
  }, []);

  const handleNavClick = (nextView: View): void => {
    setView(nextView);
    setSidebarOpen(false);
  };

  // 只处理队首(先进先出);dismiss 弹掉当前请求,deny 语义由弹窗自己发送
  const front = queue[0];
  const dismissFront = (): void => setQueue((q) => q.slice(1));

  return (
    <div className="app-root">
      <KillSwitchBar
        isNarrow={isNarrow}
        onToggleSidebar={() => setSidebarOpen((o) => !o)}
      />
      <ModelDownloadBar />
      <div className="app-body">
        <nav
          className={`sidebar ${isNarrow ? "narrow" : ""} ${sidebarOpen ? "open" : ""}`}
          aria-label="主导航"
          role={isNarrow && sidebarOpen ? "dialog" : undefined}
          aria-modal={isNarrow && sidebarOpen ? "true" : undefined}
        >
          <h1 className="app-title">VoicePilot</h1>
          <ul className="nav-list" role="list">
            {NAV_ITEMS.map((item) => (
              <li key={item.view}>
                <button
                  type="button"
                  data-view={item.view}
                  className={`nav-item ${view === item.view ? "active" : ""}`}
                  onClick={() => handleNavClick(item.view)}
                  aria-pressed={view === item.view}
                  aria-current={view === item.view ? "page" : undefined}
                >
                  <Icon name={item.icon} className="nav-icon" />
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        {isNarrow && sidebarOpen && (
          <div
            className="sidebar-backdrop"
            onClick={() => setSidebarOpen(false)}
            aria-hidden="true"
          />
        )}
        <main className="main-content">
          {view === "main" && <MainView />}
          {view === "settings" && <SettingsView />}
          {view === "audit" && <AuditViewerView />}
          {view === "trust" && <TrustCenterView />}
          {view === "skills" && <SkillsManagerView />}
          {view === "dag-history" && <DagHistoryView />}
        </main>
      </div>
      {front && front.kind === "approval" && (
        <ApprovalModal payload={front.payload} onDismiss={dismissFront} />
      )}
      {front && front.kind === "dag" && (
        <DagApprovalDialog payload={front.payload} onDismiss={dismissFront} />
      )}
      {front && front.kind === "clarify" && (
        <ClarificationDialog payload={front.payload} onDismiss={dismissFront} />
      )}
    </div>
  );
}
