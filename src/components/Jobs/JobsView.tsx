import { useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronRight, Clock3, Network, Radar } from "lucide-react";
import { describeSchedule, formatFuture } from "../../lib/formatters";
import type { CronJob } from "../../lib/types";
import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { CronWorkspaceView } from "../Cron/CronWorkspaceView";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { AutomationNotices, useScenarioCameras } from "../Automation/AutomationGraphView";
import { focusEntry, focusLost } from "../Automation/focus";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { ScenarioPanel } from "../Automation/ScenarioPanel";
import { sentinelJobs, type SentinelJob } from "../Automation/scenarios";
import { AUTOMATION_ROW, stateTone } from "../Automation/styles";
import type { AutomationAttachment, AutomationGraphAdapter } from "../Automation/types";
import { useAutomationGraph } from "../Automation/useAutomationGraph";
import { useCronJobs } from "./useCronJobs";

export interface JobsTarget {
  teamId: string | null;
  scenarioId: string;
}

type Selection = { kind: "cron" | "scenario"; id: string } | null;

interface JobsViewProps {
  workspaceId: string;
  teamId: string | null;
  adapter?: AutomationGraphAdapter;
  initialScenarioId?: string;
  cronActionAdapter?: MeshResourceActionAdapter;
}

function JobRow({ jobKey, icon, name, meta, type, state, onOpen }: { jobKey: string; icon: ReactNode; name: string; meta: string; type: string; state: string; onOpen: () => void }) {
  return <li><button type="button" data-job-key={jobKey} className={AUTOMATION_ROW} onClick={onOpen}>
    <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-[var(--ct-titlebar-bg)] text-[var(--ct-muted-fg)]">{icon}</span>
    <span className="min-w-0 flex-1">
      <span className="block truncate text-[length:var(--ct-font-size-sm)] font-medium">{name}</span>
      <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{meta}</span>
    </span>
    <span className="flex shrink-0 flex-col items-end gap-1">
      <span className="rounded-full border border-[var(--ct-border)] px-2 py-px text-[0.6875rem] text-[var(--ct-muted-fg)]">{type}</span>
      <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
    </span>
    <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" aria-hidden="true" />
  </button></li>;
}

function cronMeta(job: CronJob): string {
  const schedule = describeSchedule(job.schedule);
  return job.enabled && job.next_run ? `${schedule} · next ${formatFuture(job.next_run)}` : schedule;
}

function sentinelMeta(job: SentinelJob): string {
  return job.root?.purpose ?? job.scenario.root.id;
}

function JobsSurface({ adapter, attachment, initialScenarioId, cronActionAdapter }: { adapter: AutomationGraphAdapter; attachment: AutomationAttachment; initialScenarioId?: string; cronActionAdapter?: MeshResourceActionAdapter }) {
  const automation = useAutomationGraph(adapter, attachment);
  const crons = useCronJobs(attachment.workspace_id);
  const [selection, setSelection] = useState<Selection>(initialScenarioId ? { kind: "scenario", id: initialScenarioId } : null);
  const cameraFor = useScenarioCameras();
  const list = useRef<HTMLDivElement>(null);
  const removed = useRef<HTMLParagraphElement>(null);
  const returnTo = useRef<string | null>(null);
  const scopedCrons = useMemo(() => crons.jobs?.filter((job) => job.workspace_id === attachment.workspace_id && job.team_id === attachment.team_id) ?? null, [crons.jobs, attachment.workspace_id, attachment.team_id]);
  const sentinels = automation.graph ? sentinelJobs(automation.graph, attachment) : null;
  const scenario = selection?.kind === "scenario" ? automation.graph?.scenarios.find((entry) => entry.id === selection.id) : undefined;
  const missing = selection?.kind === "scenario" && automation.graph !== null && !scenario;
  const back = (key: string) => { returnTo.current = key; setSelection(null); };

  useLayoutEffect(() => {
    if (selection && !missing) return;
    if (returnTo.current) focusEntry(list.current, "jobKey", returnTo.current);
    else if (missing && focusLost()) removed.current?.focus();
    returnTo.current = null;
  }, [selection, missing]);

  if (selection?.kind === "cron") {
    const id = selection.id;
    return <CronWorkspaceView cronId={id} actionAdapter={cronActionAdapter} backLabel="All jobs" onBack={() => { void crons.refresh(); back(`cron:${id}`); }} />;
  }
  if (scenario) {
    return <ScenarioPanel state={automation} scope={attachment} scenario={scenario} camera={cameraFor(scenario.id)} backLabel="All jobs" headingLevel={3} fill onBack={() => back(`scenario:${scenario.id}`)}>
      <AutomationNotices state={automation} noun="Sentinels" />
    </ScenarioPanel>;
  }
  const reading = (crons.jobs === null && !crons.failed) || automation.loading;
  const empty = scopedCrons?.length === 0 && sentinels?.length === 0 && !crons.failed && !automation.failed;
  return <div ref={list} className="@container mx-auto w-full max-w-4xl px-4 py-4 sm:px-6">
    {missing && <p ref={removed} tabIndex={-1} role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2 outline-none">The selected Sentinel is unavailable. It may have been removed; choose another job.</p>}
    {crons.failed && <p role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2">Cron jobs unavailable. {scopedCrons ? "Showing the last list read." : "No cron records could be read."}</p>}
    {!automation.loading && <AutomationNotices state={automation} noun="Sentinels" />}
    {reading && <p role="status" className="mb-3 text-[var(--ct-muted-fg)]">Reading jobs…</p>}
    {empty && <p data-testid="jobs-empty" className="mb-3 text-[var(--ct-muted-fg)]">No jobs in this scope.</p>}
    <ul aria-label="Jobs" className="grid gap-2 @3xl:grid-cols-2">
      {scopedCrons?.map((job) => <JobRow key={`cron:${job.id}`} jobKey={`cron:${job.id}`} icon={<Clock3 size={15} aria-hidden="true" />} name={job.name} meta={cronMeta(job)} type="Cron" state={job.enabled ? "active" : "paused"} onOpen={() => setSelection({ kind: "cron", id: job.id })} />)}
      {sentinels?.map((job) => <JobRow key={`scenario:${job.scenario.id}`} jobKey={`scenario:${job.scenario.id}`} icon={job.type === "Sentinel graph" ? <Network size={15} aria-hidden="true" /> : <Radar size={15} aria-hidden="true" />} name={job.scenario.name} meta={sentinelMeta(job)} type={job.type} state={job.state} onOpen={() => setSelection({ kind: "scenario", id: job.scenario.id })} />)}
    </ul>
  </div>;
}

export function JobsView({ workspaceId, teamId, adapter = hostAutomationAdapter, initialScenarioId, cronActionAdapter }: JobsViewProps) {
  const context = usePlanPeerContext();
  if (!context.ready) return <p role="status" className="p-4">Attaching jobs…</p>;
  const attachment = { workspace_id: workspaceId, team_id: teamId, connection_id: `host:${context.generation}` };
  return <JobsSurface key={JSON.stringify([attachment.connection_id, workspaceId, teamId])} adapter={adapter} attachment={attachment} initialScenarioId={initialScenarioId} cronActionAdapter={cronActionAdapter} />;
}
