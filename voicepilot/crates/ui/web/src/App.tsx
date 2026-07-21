import { useEffect, useState } from "react";
import { onApprovalRequest } from "./api";
import type { ApprovalRequestPayload, View } from "./types";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { SettingsView } from "./components/SettingsView";
import { AuditViewerView } from "./components/AuditViewerView";
import { TrustCenterView } from "./components/TrustCenterView";
import { SkillsManagerView } from "./components/SkillsManagerView";
import { KillSwitchBar } from "./components/KillSwitchBar";

const NAV_ITEMS: { view: View; label: string }[] = [
  { view: "main", label: "Main Chat" },
  { view: "settings", label: "Settings" },
  { view: "audit", label: "Audit Viewer" },
  { view: "trust", label: "Trust Center" },
  { view: "skills", label: "Skills Manager" },
];

const NARROW_BREAKPOINT = 768;

export function App(): JSX.Element {
  const [view, setView] = useState<View>("main");
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);
  // 响应式布局(W6a Fast-Follow):窄窗口隐藏 sidebar,改为 overlay
  const [isNarrow, setIsNarrow] = useState<boolean>(window.innerWidth < NARROW_BREAKPOINT);
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(false);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => setApproval(payload));
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

  const handleNavClick = (nextView: View): void => {
    setView(nextView);
    setSidebarOpen(false);
  };

  return (
    <div className="app-root">
      <KillSwitchBar
        isNarrow={isNarrow}
        onToggleSidebar={() => setSidebarOpen((o) => !o)}
      />
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
                  className={`nav-item ${view === item.view ? "active" : ""}`}
                  onClick={() => handleNavClick(item.view)}
                  aria-pressed={view === item.view}
                  aria-current={view === item.view ? "page" : undefined}
                >
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
        </main>
      </div>
      {approval && (
        <ApprovalModal
          payload={approval}
          onDismiss={() => setApproval(null)}
        />
      )}
    </div>
  );
}
