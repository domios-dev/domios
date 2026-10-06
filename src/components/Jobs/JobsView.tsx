import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Clock3, Network, Radar, Square, Trash2, Workflow, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import type { CronJob, Team } from "../../lib/types";
import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { CronWorkspaceView } from "../Cron/CronWorkspaceView";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { AutomationNotices, useScenarioCameras } from "../Automation/AutomationGraphView";
import { focusEntry, focusLost } from "../Automation/focus";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { ScenarioPanel } from "../Automation/ScenarioPanel";
import { sentinelJobs } from "../Automation/scenarios";
import { AUTOMATION_TOOL } from "../Automation/styles";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "./JobConfirmationDialog";
import { cronMeta, JobRow, sentinelMeta, unnamedGraphId } from "./JobRow";
import { JobsFilterBar, type JobsWorkspace } from "./JobsFilterBar";
import { useCronJobs } from "./useCronJobs";
import { useWorkspaceGraphs } from "./useWorkspaceGraphs";
import { clearableJobs, cronState, mergeJobs, NO_JOBS_FILTER, type JobsFilter, type MergedJob } from "./jobsModel";

export interface JobsTarget {
  teamId: string | null;
  scenarioId: string;
}

/** `workspace` is the overlay opened from a workspace; `settings` is the machine-wide entry. */
export type JobsPlacement = "workspace" | "settings";

type Selection = { kind: "cron" | "scenario"; id: string } | null;

export interface JobsActionAdapter {
  stopCron(job: CronJob): Promise<unknown>;
  deleteCron(job: CronJob): Promise<unknown>;
}

const hostJobsActionAdapter: JobsActionAdapter = {
  stopCron: (job) => invoke("cron_cancel", { id: job.id }),
  deleteCron: (job) => invoke("cron_delete", { id: job.id }),
};

type Confirmation = { kind: "delete"; job: MergedJob } | { kind: "clear"; jobs: MergedJob[] };

function jobKey(job: MergedJob): string {
  return job.kind === "cron" ? `cron:${job.cron.id}` : `scenario:${job.sentinel.scenario.id}`;
}

function jobWorkspace(job: MergedJob): string {
  return job.kind === "cron" ? job.cron.workspace_id : job.sentinel.scenario.root.workspace_id;
}

function EmptyJobs({ filter, placement }: { filter: JobsFilter; placement: JobsPlacement }) {
  const scoped = filter.workspaceId !== null || filter.teamId !== null;
  const what = filter.teamId !== null ? "Crons and graphs of this Team will appear here."
    : filter.workspaceId !== null ? "Crons and graphs of this workspace will appear here."
      : "Crons and graphs on this machine will appear here.";
  const hint = !scoped ? null : placement === "workspace" ? "Every job on this machine is in Settings › Jobs." : "Remove the filters to see every job on this machine.";
  return <>
    <Workflow size={28} className="text-[var(--ct-muted)]" aria-hidden="true" />
    <p className="font-medium text-[var(--ct-fg)]">No jobs</p>
    <p className="max-w-sm text-[var(--ct-muted-fg)]">{what}</p>
    {hint && <p className="max-w-sm text-xs text-[var(--ct-muted)]">{hint}</p>}
  </>;
}

interface SurfaceProps {
  adapter: AutomationGraphAdapter;
  connectionId: string;
  sources: readonly string[];
  cronWorkspace: string | null;
  filter: JobsFilter;
  placement: JobsPlacement;
  initialScenarioId?: string;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter: JobsActionAdapter;
  onClearStoppedChange: (count: number, onClear: (() => void) | null) => void;
  clearStoppedButtonRef: RefObject<HTMLButtonElement | null>;
}

function JobsSurface({ adapter, connectionId, sources, cronWorkspace, filter, placement, initialScenarioId, cronActionAdapter, jobActionAdapter, onClearStoppedChange, clearStoppedButtonRef }: SurfaceProps) {
  const graphs = useWorkspaceGraphs(adapter, connectionId, sources);
  const crons = useCronJobs(cronWorkspace);
  const [selection, setSelection] = useState<Selection>(initialScenarioId ? { kind: "scenario", id: initialScenarioId } : null);
  const cameraFor = useScenarioCameras();
  const list = useRef<HTMLDivElement>(null);
  const removed = useRef<HTMLParagraphElement>(null);
  const returnTo = useRef<string | null>(null);
  const menuTriggers = useRef(new Map<string, HTMLButtonElement>());
  const dialogFocus = useRef<HTMLElement | null>(null);
  const dialogOpen = useRef(false);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [confirmationBusy, setConfirmationBusy] = useState(false);
  const [confirmationError, setConfirmationError] = useState<string | null>(null);
  const [stoppingJob, setStoppingJob] = useState<string | null>(null);
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({});
  const graphsRead = graphs.every((graph) => graph.graph !== null);
  const graphsFailed = graphs.some((graph) => graph.failed);
  const sentinels = useMemo(() => graphs.flatMap((graph) => graph.graph ? sentinelJobs(graph.graph, graph.attachment) : []), [graphs]);
  const jobs = useMemo(() => mergeJobs(filter, crons.jobs ?? [], sentinels), [filter, crons.jobs, sentinels]);
  const clearable = useMemo(() => clearableJobs(jobs), [jobs]);
  const canClear = graphsRead && crons.jobs !== null && !graphsFailed && !crons.failed;
  const graphFor = useCallback((workspaceId: string) => graphs.find((graph) => graph.attachment.workspace_id === workspaceId), [graphs]);
  const opened = useMemo(() => {
    if (selection?.kind !== "scenario") return undefined;
    for (const graph of graphs) {
      const scenario = graph.graph?.scenarios.find((entry) => entry.id === selection.id);
      if (scenario) return { graph, scenario };
    }
    return undefined;
  }, [graphs, selection]);
  const missing = selection?.kind === "scenario" && graphsRead && !opened;
  const notices = useMemo(() => ({ graph: graphs.find((graph) => graph.graph?.truncated)?.graph ?? null, loading: false, failed: graphsFailed, stopping: null, stopFailed: null }), [graphs, graphsFailed]);
  const refreshAll = useCallback(() => Promise.all([crons.refresh(), ...graphs.map((graph) => graph.refresh())]), [crons.refresh, graphs]);
  const back = (key: string) => { returnTo.current = key; setSelection(null); };

  const requestClearStopped = useCallback(() => {
    if (!canClear || clearable.length === 0) return;
    dialogFocus.current = clearStoppedButtonRef.current;
    setConfirmationError(null);
    setConfirmation({ kind: "clear", jobs: clearable });
  }, [canClear, clearable, clearStoppedButtonRef]);

  useEffect(() => {
    onClearStoppedChange(canClear ? clearable.length : 0, canClear && clearable.length > 0 ? requestClearStopped : null);
    return () => onClearStoppedChange(0, null);
  }, [canClear, clearable.length, onClearStoppedChange, requestClearStopped]);

  useLayoutEffect(() => {
    if (confirmation) {
      dialogOpen.current = true;
      return;
    }
    if (!dialogOpen.current) return;
    dialogOpen.current = false;
    const target = dialogFocus.current;
    dialogFocus.current = null;
    if (target?.isConnected) target.focus();
    else list.current?.querySelector<HTMLButtonElement>("button[data-job-key]")?.focus();
  }, [confirmation, jobs]);

  const deleteJob = useCallback(async (job: MergedJob) => {
    if (job.kind === "cron") return jobActionAdapter.deleteCron(job.cron);
    const graph = graphFor(jobWorkspace(job));
    if (!graph) throw new Error("Sentinel workspace is not attached");
    return adapter.delete(graph.attachment, job.sentinel.scenario.root);
  }, [adapter, graphFor, jobActionAdapter]);

  const stopJob = useCallback(async (job: MergedJob) => {
    const key = jobKey(job);
    setStoppingJob(key);
    setActionErrors((current) => ({ ...current, [key]: "" }));
    try {
      if (job.kind === "cron") {
        await jobActionAdapter.stopCron(job.cron);
        await crons.refresh();
      } else {
        const root = job.sentinel.root;
        const graph = graphFor(jobWorkspace(job));
        if (!root || !graph) return;
        await adapter.stop(graph.attachment, root.identity);
        await graph.refresh();
      }
    } catch {
      setActionErrors((current) => ({ ...current, [key]: `Could not stop ${job.title}.` }));
    } finally {
      setStoppingJob(null);
    }
  }, [adapter, crons.refresh, graphFor, jobActionAdapter]);

  const beginDelete = (job: MergedJob) => {
    dialogFocus.current = menuTriggers.current.get(jobKey(job)) ?? null;
    setConfirmationError(null);
    setConfirmation({ kind: "delete", job });
  };

  const closeConfirmation = () => {
    if (confirmationBusy) return;
    setConfirmation(null);
    setConfirmationError(null);
  };

  const confirmDelete = async () => {
    if (!confirmation) return;
    const targets = confirmation.kind === "delete" ? [confirmation.job] : confirmation.jobs;
    setConfirmationBusy(true);
    setConfirmationError(null);
    const results = await Promise.allSettled(targets.map((job) => deleteJob(job)));
    await refreshAll();
    if (results.some((result) => result.status === "rejected")) {
      if (confirmation.kind === "clear") {
        setConfirmation({ kind: "clear", jobs: targets.filter((_, index) => results[index].status === "rejected") });
      }
      setConfirmationBusy(false);
      setConfirmationError(confirmation.kind === "delete"
        ? `Could not delete ${confirmation.job.title}.`
        : "Some stopped jobs could not be deleted. The list has been refreshed.");
      return;
    }
    setConfirmationBusy(false);
    setConfirmation(null);
  };

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
  if (opened) {
    const { graph, scenario } = opened;
    return <ScenarioPanel state={graph} scope={graph.attachment} scenario={scenario} camera={cameraFor(scenario.id)} backLabel="All jobs" headingLevel={3} fill onBack={() => back(`scenario:${scenario.id}`)}>
      <AutomationNotices state={graph} noun="Sentinels" />
    </ScenarioPanel>;
  }
  const reading = (crons.jobs === null && !crons.failed) || graphs.some((graph) => graph.loading);
  const empty = jobs.length === 0 && crons.jobs !== null && graphsRead && !crons.failed && !graphsFailed;
  return <div ref={list} className="@container mx-auto flex w-full max-w-4xl flex-1 flex-col px-4 py-4 sm:px-6">
    {missing && <p ref={removed} tabIndex={-1} role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2 outline-none">The selected Sentinel is unavailable. It may have been removed; choose another job.</p>}
    {crons.failed && <p role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2">Cron jobs unavailable. {crons.jobs ? "Showing the last list read." : "No cron records could be read."}</p>}
    <AutomationNotices state={notices} noun="Sentinels" />
    {reading && <p role="status" className="mb-3 text-[var(--ct-muted-fg)]">Reading jobs…</p>}
    <div role="status" aria-label={empty ? "No jobs" : undefined} className={empty ? "flex flex-1 flex-col items-center justify-center gap-1.5 py-10 text-center" : undefined}>
      {empty && <EmptyJobs filter={filter} placement={placement} />}
    </div>
    {!empty && <ul aria-label="Jobs" className="grid gap-2 @3xl:grid-cols-2">
      {jobs.map((job: MergedJob) => {
        const key = jobKey(job);
        const canStop = job.kind === "cron"
          ? cronState(job.cron) === "Running"
          : ["active", "running", "finishing"].includes(job.state.toLowerCase()) && Boolean(job.sentinel.root?.stop.allowed);
        const actionItems = [
          ...(canStop ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stoppingJob === key, onClick: () => void stopJob(job) }] : []),
          { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => beginDelete(job) },
        ];
        const onTriggerRef = (element: HTMLButtonElement | null) => { if (element) menuTriggers.current.set(key, element); else menuTriggers.current.delete(key); };
        return job.kind === "cron"
          ? <JobRow key={key} jobKey={key} icon={<Clock3 size={15} aria-hidden="true" />} name={job.title} meta={cronMeta(job.cron)} type="Cron" state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "cron", id: job.cron.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />
          : <JobRow key={key} jobKey={key} icon={job.sentinel.type === "Sentinel graph" ? <Network size={15} aria-hidden="true" /> : <Radar size={15} aria-hidden="true" />} name={job.title} meta={sentinelMeta(job.sentinel)} detail={unnamedGraphId(job.sentinel, job.title)} type={job.sentinel.type} state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "scenario", id: job.sentinel.scenario.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />;
      })}
    </ul>}
    {confirmation && <JobConfirmationDialog
      title={confirmation.kind === "delete" ? `Delete ${confirmation.job.title}?` : "Clear stopped jobs?"}
      description={confirmation.kind === "delete"
        ? `Delete ${confirmation.job.title} and its history? This cannot be undone.`
        : `Delete ${confirmation.jobs.length} stopped or disabled jobs in the current filter, including their history? This cannot be undone.`}
      confirmLabel={confirmation.kind === "delete" ? "Delete" : "Clear stopped"}
      busy={confirmationBusy}
      error={confirmationError}
      onCancel={closeConfirmation}
      onConfirm={() => void confirmDelete()}
    />}
  </div>;
}

export interface JobsViewProps {
  workspaces: readonly JobsWorkspace[];
  teams: readonly Team[];
  placement: JobsPlacement;
  initialFilter?: JobsFilter;
  initialScenarioId?: string;
  onClose?: () => void;
  adapter?: AutomationGraphAdapter;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

/** The one Jobs view: crons and Sentinel graphs narrowed by optional workspace and Team capsules. */
export function JobsView({ workspaces, teams, placement, initialFilter = NO_JOBS_FILTER, initialScenarioId, onClose, adapter = hostAutomationAdapter, cronActionAdapter, jobActionAdapter = hostJobsActionAdapter }: JobsViewProps) {
  const context = usePlanPeerContext();
  const [filter, setFilter] = useState<JobsFilter>(initialFilter);
  const [initialScenario, setInitialScenario] = useState(initialScenarioId);
  const clearStoppedButton = useRef<HTMLButtonElement>(null);
  const [clearState, setClearState] = useState<{ count: number; run: (() => void) | null }>({ count: 0, run: null });
  const setClearStopped = useCallback((count: number, run: (() => void) | null) => setClearState({ count, run }), []);
  const changeFilter = useCallback((next: JobsFilter) => { setInitialScenario(undefined); setFilter(next); }, []);
  const team = filter.teamId === null ? null : teams.find((entry) => entry.id === filter.teamId && !entry.remote_peer_name) ?? null;
  const teamAvailable = filter.teamId === null || team !== null;
  const cronWorkspace = filter.workspaceId ?? team?.workspace_id ?? null;
  const sources = useMemo(() => cronWorkspace !== null ? [cronWorkspace] : workspaces.map((workspace) => workspace.id), [cronWorkspace, workspaces]);
  const connectionId = `host:${context.ready ? context.generation : "detached"}`;
  const labelled = placement === "workspace" ? { "aria-label": "Jobs" } : {};
  return <section {...labelled} className="flex h-full min-h-0 flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
    <header className={`flex flex-wrap items-center gap-x-3 gap-y-2 ${placement === "workspace" ? "border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-3 sm:px-6" : "pb-1"}`}>
      {placement === "workspace" && <>
        <Workflow size={18} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
        <h2 className="text-[length:var(--ct-font-size-md)] font-semibold">Jobs</h2>
      </>}
      <JobsFilterBar filter={filter} workspaces={workspaces} teams={teams} onChange={changeFilter} />
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <button ref={clearStoppedButton} type="button" className={AUTOMATION_TOOL} aria-label={`Clear stopped${clearState.count ? `, ${clearState.count} jobs` : ""}`} title="Clear stopped" disabled={!clearState.run} onClick={() => clearState.run?.()}>
          Clear stopped{clearState.count > 0 && <span className="rounded border border-[var(--ct-border)] px-1.5 py-px text-[0.6875rem]">{clearState.count}</span>}
        </button>
        {onClose && <button type="button" className={AUTOMATION_TOOL} aria-label="Close jobs" title="Close jobs" onClick={onClose}><X size={15} aria-hidden="true" /></button>}
      </div>
    </header>
    <div className={`flex min-h-0 flex-1 flex-col ${placement === "workspace" ? "overflow-y-auto" : "min-h-[18rem]"}`}>
      {!context.ready ? <p role="status" className="p-4">Attaching jobs…</p>
        : !teamAvailable ? <p role="alert" className="p-4">The selected Team is unavailable.</p>
          : <JobsSurface key={JSON.stringify([connectionId, sources, filter])} adapter={adapter} connectionId={connectionId} sources={sources} cronWorkspace={cronWorkspace} filter={filter} placement={placement} initialScenarioId={initialScenario} cronActionAdapter={cronActionAdapter} jobActionAdapter={jobActionAdapter} onClearStoppedChange={setClearStopped} clearStoppedButtonRef={clearStoppedButton} />}
    </div>
  </section>;
}
