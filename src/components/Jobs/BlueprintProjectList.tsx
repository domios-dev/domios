import { BlueprintParameters } from "../Automation/BlueprintParameters";
import { useEffect, useState, type CSSProperties } from "react";
import { FileCode2, FolderGit2 } from "lucide-react";
import type { BlueprintProjectEntry, BlueprintProjectView } from "../../lib/blueprintApi";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { BlueprintView } from "../Automation/blueprint/BlueprintView";
import { pipelineModel } from "../Automation/pipelineModel";
import { BlueprintProjectionValue } from "../Automation/BlueprintProjectionValue";
import { AUTOMATION_TONE, AUTOMATION_TOOL } from "../Automation/styles";
import { BackPill } from "../shared/BackPill";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { JobRow } from "./JobRow";

export interface BlueprintProjectApi {
  list(workspaceId: string): Promise<readonly BlueprintProjectEntry[]>;
  show(workspaceId: string, path: string): Promise<BlueprintProjectView>;
}
const hostApi: BlueprintProjectApi = {
  list: (workspaceId) => orchestrationTransport().blueprint_project_list({ workspaceId }),
  show: (workspaceId, path) => orchestrationTransport().blueprint_project_show({ workspaceId, path }),
};

type Entry = BlueprintProjectEntry & { workspaceId: string };

/** The selected workspaces' repository Blueprints as read-only rows; opening one shows its source definition. */
export function BlueprintProjectList({ workspaceIds, workspaceName = (id) => id, api = hostApi, onClose }: {
  workspaceIds: readonly string[]; workspaceName?: (id: string) => string; api?: BlueprintProjectApi; onClose?: () => void;
}) {
  const [entries, setEntries] = useState<readonly Entry[] | null>(null);
  const [view, setView] = useState<BlueprintProjectView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const scope = workspaceIds.join("\n");
  useEffect(() => {
    let live = true;
    setEntries(null); setView(null); setError(null);
    void Promise.all(scope.split("\n").filter(Boolean).map(async (workspaceId) => (await api.list(workspaceId)).map((entry) => ({ ...entry, workspaceId }))))
      .then((lists) => { if (live) setEntries(lists.flat()); }, (failure: unknown) => { if (live) setError(failure); });
    return () => { live = false; };
  }, [scope, api]);
  const open = async (entry: Entry) => {
    setBusy(true); setError(null);
    try { setView(await api.show(entry.workspaceId, entry.path)); } catch (failure: unknown) { setError(failure); } finally { setBusy(false); }
  };
  const several = workspaceIds.length > 1;
  const pipeline = view ? pipelineModel({ id: view.path, name: view.document.name, state: "idle", run_id: null, current_step: null, last_event_at_ms: null, call_graph: view.document }) : null;
  return <section aria-label="Repository Blueprints" className="ct-card ct-card--static" style={{ "--ct-card-tone": AUTOMATION_TONE.blueprint } as CSSProperties}>
    <div className="ct-card__head">
      <span className="ct-card__icon" aria-hidden="true"><FolderGit2 size={14} /></span>
      <h3 className="ct-detail__heading">Repository Blueprints</h3>
      {onClose && <button type="button" className={`${AUTOMATION_TOOL} ml-auto`} onClick={onClose}>Close</button>}
    </div>
    <div className="ct-card__body ct-job-section__body">
      {view ? <section aria-label={`Repository source ${view.path}`} className="flex min-w-0 flex-col gap-3">
        <BackPill label="Repository Blueprints" onBack={() => setView(null)} />
        {view.document.intent && <p className="ct-job-note">{view.document.intent}</p>}
        <BlueprintParameters key={view.path} inputs={view.document.inputs ?? {}} />
        {pipeline && <BlueprintView pipeline={pipeline} headingLevel={3} />}
        <BlueprintProjectionValue value={view.document} label="Source definition" />
      </section>
        : entries === null ? error == null && <p role="status" className="ct-job-note">Reading repository Blueprints…</p>
        : entries.length === 0 ? <p className="ct-job-note">No Blueprints in {several ? "these repositories" : "this repository"}.</p>
        : <ul aria-label="Repository Blueprint list" className="flex flex-col gap-2">{entries.map((entry) => <JobRow key={`${entry.workspaceId}:${entry.path}`}
          jobKey={`repository:${entry.workspaceId}:${entry.path}`} icon={<FileCode2 size={15} aria-hidden="true" />} name={entry.name ?? entry.path}
          meta={entry.error ? `Unreadable: ${entry.error}` : [entry.path, entry.intent, several ? workspaceName(entry.workspaceId) : null].filter(Boolean).join(" · ")}
          type="Repository Blueprint" tone="blueprint" state={entry.error ? "Unreadable" : "Source"} actionItems={[]}
          onOpen={() => { if (!entry.error && !busy) void open(entry); }} onTriggerRef={() => {}} error={null} />)}</ul>}
      {error != null && <InlineErrorNotice error={error} operation="reading repository Blueprints" blocking />}
    </div>
  </section>;
}
