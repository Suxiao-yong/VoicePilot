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

export function App(): JSX.Element {
  const [view, setView] = useState<View>("main");
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => setApproval(payload));
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return (
    <div className="app-root">
      <KillSwitchBar />
      <div className="app-body">
        <nav className="sidebar" aria-label="主导航">
          <h1 className="app-title">VoicePilot</h1>
          <ul className="nav-list" role="list">
            {NAV_ITEMS.map((item) => (
              <li key={item.view}>
                <button
                  type="button"
                  className={`nav-item ${view === item.view ? "active" : ""}`}
                  onClick={() => setView(item.view)}
                  aria-pressed={view === item.view}
                  aria-current={view === item.view ? "page" : undefined}
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
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
