import { BlueprintProjectionValue } from "./BlueprintProjectionValue";
import { useEffect, useState } from "react";
import type { BlueprintViewerHistory } from "../../lib/blueprintApi";
import { blueprintDiffModel } from "./blueprintDiffModel";

export function BlueprintSemanticDiff({ load }: { load: () => Promise<BlueprintViewerHistory> }) {
  const [history, setHistory] = useState<BlueprintViewerHistory | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<number | null>(null);
  useEffect(() => {
    let active = true;
    void load().then((history) => { if (active) { setHistory(history); setVersion(history.current_version); } }).catch((error: unknown) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [load]);
  const selected = history?.versions.find((entry) => entry.version === version);
  const rows = selected?.diff ? blueprintDiffModel(selected.diff) : [];
  return <section aria-label="Semantic diff" className="ct-detail__block">
    {error && <p role="alert">{error}</p>}
    {!history && !error && <p role="status">Reading definition history…</p>}
    {history && <>
      <label>Definition version <select aria-label="Definition version" value={version ?? ""} onChange={(event) => setVersion(Number(event.target.value))}>{history.versions.map((entry) => <option key={entry.version} value={entry.version}>{entry.version}</option>)}</select></label>
      {history.history_truncated && <p role="status">Earlier definition history is unavailable.</p>}
      <p>{selected?.replaces != null ? `Version ${selected.replaces} → ${selected.version}` : "Initial retained definition"}</p>
      {rows.length > 0 && <table><caption>Semantic changes</caption><thead><tr><th>Graph</th><th>Node</th><th>Change</th><th>Field</th></tr></thead><tbody>{rows.map((row, index) => <tr key={index}><td>{row.scope}</td><td>{row.node ?? "—"}</td><td>{row.kind}</td><td>{row.field}</td></tr>)}</tbody></table>}
      {selected?.before && <BlueprintProjectionValue value={selected.before} label="Before definition and callees" />}
      {selected?.after && <BlueprintProjectionValue value={selected.after} label="After definition and callees" />}
      {selected?.replaces != null && !selected.diff && <p role="status">Predecessor definition unavailable; comparison cannot be shown.</p>}
      {selected?.diff && rows.length === 0 && <p role="status">No semantic changes.</p>}
    </>}
  </section>;
}
