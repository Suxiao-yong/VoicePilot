import { useState } from "react";
import { routeText, organizeFiles } from "../api";
import type { RouteTextResult, OrganizeResult } from "../types";

export function MainView() {
  const [text, setText] = useState("");
  const [routeResult, setRouteResult] = useState<RouteTextResult | null>(null);
  const [source, setSource] = useState("");
  const [filter, setFilter] = useState("*.txt");
  const [destination, setDestination] = useState("");
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(null);
  const [busy, setBusy] = useState(false);

  async function onRoute() {
    setBusy(true);
    try {
      const r = await routeText(text);
      setRouteResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  async function onOrganize() {
    setBusy(true);
    try {
      const r = await organizeFiles({
        task_id: `t-${Date.now()}`,
        step_id: `s-${Date.now()}`,
        source,
        filter,
        destination,
      });
      setOrganizeResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">§ 5.1 Skill Router</div>
      <h1 className="panel-title">
        Route <em>intent</em> → Skill
      </h1>

      <div className="form-row">
        <label>Text</label>
        <input
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="整理下载目录"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onRoute} disabled={busy}>
          Route
        </button>
      </div>

      {routeResult && (
        <div
          className={`route-result ${
            routeResult.kind === "routed" ? "routed" : "unmatched"
          }`}
        >
          {routeResult.kind === "routed" && (
            <>✓ Routed to skill: <strong>{routeResult.skill_id}</strong></>
          )}
          {routeResult.kind === "unmatched" && (
            <>? No skill matched: <strong>{routeResult.text}</strong></>
          )}
          {routeResult.kind === "empty" && <>∅ Empty input</>}
        </div>
      )}

      <div className="panel-header" style={{ marginTop: 48 }}>§ 5.2 Files Organize</div>
      <h1 className="panel-title">
        Run <em>files.organize</em>
      </h1>

      <div className="form-row">
        <label>Source</label>
        <input
          type="text"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          placeholder="D:/Downloads"
        />
      </div>
      <div className="form-row">
        <label>Filter</label>
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="*.pdf"
        />
      </div>
      <div className="form-row">
        <label>Destination</label>
        <input
          type="text"
          value={destination}
          onChange={(e) => setDestination(e.target.value)}
          placeholder="D:/Documents/Papers"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onOrganize} disabled={busy}>
          Organize
        </button>
      </div>

      {organizeResult && (
        <div className="route-result routed">
          <div>
            committed: <strong>{String(organizeResult.committed)}</strong>
          </div>
          <div>
            moved: <strong>{organizeResult.moved_paths.length}</strong> file(s)
          </div>
          <div>
            evidence: <strong>{organizeResult.evidence_strength}</strong>
          </div>
          {organizeResult.compensation_ref && (
            <div>
              compensation_ref: <strong>{organizeResult.compensation_ref}</strong>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
