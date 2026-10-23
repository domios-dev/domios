import { BlueprintParameters } from "../Automation/BlueprintParameters";
import { useState } from "react";
import type { BlueprintProjectEntry, BlueprintProjectView } from "../../lib/blueprintApi";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { BlueprintPipelineCanvas } from "../Automation/BlueprintPipelineCanvas";
import { pipelineModel } from "../Automation/pipelineModel";
import { BlueprintProjectionValue } from "../Automation/BlueprintProjectionValue";

export interface BlueprintProjectApi {
  list(workspaceId: string): Promise<readonly BlueprintProjectEntry[]>;
  show(workspaceId: string, path: string): Promise<BlueprintProjectView>;
}
const hostApi: BlueprintProjectApi = {
  list: (workspaceId) => orchestrationTransport().blueprint_project_list({ workspaceId }),
  show: (workspaceId, path) => orchestrationTransport().blueprint_project_show({ workspaceId, path }),
};
export function BlueprintProjectList({ workspaceId, api = hostApi }: { workspaceId: string; api?: BlueprintProjectApi }) {
  const [entries, setEntries] = useState<readonly BlueprintProjectEntry[] | null>(null);
  const [view, setView] = useState<BlueprintProjectView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = async (path?: string) => {
    setBusy(true); setError(null);
    try {
      if (path) setView(await api.show(workspaceId, path));
      else { setEntries(await api.list(workspaceId)); setView(null); }
    } catch (error: unknown) { setError(String(error)); }
    finally { setBusy(false); }
  };
  const pipeline = view ? pipelineModel({ id: view.path, name: view.document.name, state: "idle", run_id: null, current_step: null, last_event_at_ms: null, call_graph: view.document }) : null;
  return <section aria-label="Repository Blueprints">
    <button type="button" disabled={busy} onClick={() => void load()}>List repository Blueprints</button>
    {error && <p role="alert">{error}</p>}
    {entries && <ul>{entries.map((entry) => <li key={entry.path}>
      <dl><dt>Path</dt><dd>{entry.path}</dd><dt>Name</dt><dd>{entry.name ?? "unavailable"}</dd><dt>Intent</dt><dd>{entry.intent ?? "unavailable"}</dd></dl>
      {entry.error ? <p role="status">{entry.error}</p> : <button type="button" aria-label={`View ${entry.path}`} disabled={busy} onClick={() => void load(entry.path)}>View</button>}
    </li>)}</ul>}
    {view && <section aria-label={`Repository source ${view.path}`}>
      <h3>{view.document.name}</h3><p>{view.document.intent}</p>
      <BlueprintParameters key={view.path} inputs={view.document.inputs ?? {}} />
      {pipeline && <BlueprintPipelineCanvas pipeline={pipeline} />}
      <BlueprintProjectionValue value={view.document} label="Source definition" />
    </section>}
  </section>;
}
