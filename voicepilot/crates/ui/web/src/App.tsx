import { useEffect, useState } from "react";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { onApprovalRequest } from "./api";
import type { ApprovalRequestPayload } from "./types";

export function App() {
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => {
      setApproval(payload);
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return (
    <div className="app-shell">
      <div className="topbar">
        <span className="brand">VoicePilot</span>
        <span>W6a · Trust Kernel · Single Rust Architecture</span>
      </div>
      <div className="main">
        <MainView />
        <div className="status-panel">
          <div className="panel-header">Kernel Status</div>
          <div className="status-line">
            <span className="key">Spec:</span>
            <span className="val">V1.1.2</span>
          </div>
          <div className="status-line">
            <span className="key">Architecture:</span>
            <span className="val">Single Rust Kernel</span>
          </div>
          <div className="status-line">
            <span className="key">Voice:</span>
            <span className="val">opt-in (W5)</span>
          </div>
          <div className="status-line">
            <span className="key">Approval TTL:</span>
            <span className="val">300s</span>
          </div>
        </div>
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
