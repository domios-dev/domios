import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Clock3, Network, Radar, Square, Trash2, Workflow, X } from "lucide-react";
import type { Tab, Team } from "../../lib/types";
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
import { groupBlueprintsByOwner, legacyScenarioPipeline, pipelineModel } from "../Automation/pipelineModel";
import { BlueprintPipelineCanvas } from "../Automation/AutomationCanvas";
import { JobConfirmationDialog } from "./JobConfirmationDialog";
import { cronMeta, JobRow, sentinelMeta, unnamedGraphId } from "./JobRow";
import { JobsFilterBar } from "./JobsFilterBar";
import { jobsLayout, type JobsEmptyScope, type JobsWorkspace } from "./jobsLayout";
import { useCronJobs } from "./useCronJobs";
import { useWorkspaceGraphs } from "./useWorkspaceGraphs";
import { deleteJobConfirmation, deleteMergedJob, hostJobsActionAdapter, stopMergedJob, type JobActionContext, type JobsActionAdapter } from "./jobActions";
import { canStopJob, clearableJobs, mergeJobs, NO_JOBS_FILTER, type JobsFilter, type MergedJob } from "./jobsModel";

export type { JobsActionAdapter } from "./jobActions";

export interface JobsTarget {
  teamId: string | null;
  scenarioId?: string;
  cronId?: string;
  blueprintId?: string;
}

type Selection = { kind: "cron" | "scenario" | "blueprint"; id: string } | null;

type Confirmation = { kind: "delete"; job: MergedJob } | { kind: "clear"; jobs: MergedJob[] };

function jobKey(job: MergedJob): string {
  return job.kind === "cron" ? `cron:${job.cron.id}` : `scenario:${job.sentinel.scenario.id}`;
}

const EMPTY_COPY: Record<JobsEmptyScope, string> = {
  team: "Crons and graphs of this Team will appear here.",
  workspace: "Crons and graphs of this workspace will appear here.",
  machine: "Crons and graphs on this machine will appear here.",
};

function EmptyJobs({ scope, canWiden }: { scope: JobsEmptyScope; canWiden: boolean }) {
  const what = EMPTY_COPY[scope];
  const hint = canWiden ? "Remove the filters to see every job on this machine." : null;
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
  emptyScope: JobsEmptyScope;
  canWiden: boolean;
  initialScenarioId?: string;
  initialCronId?: string;
  initialBlueprintId?: string;
  teams: readonly Team[];
  tabs?: Record<string, Tab>;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter: JobsActionAdapter;
  onClearStoppedChange: (count: number, onClear: (() => void) | null) => void;
  clearStoppedButtonRef: RefObject<HTMLButtonElement | null>;
}

function JobsSurface({ adapter, connectionId, sources, cronWorkspace, filter, emptyScope, canWiden, initialScenarioId, initialCronId, initialBlueprintId, teams, tabs = {}, cronActionAdapter, jobActionAdapter, onClearStoppedChange, clearStoppedButtonRef }: SurfaceProps) {
  const graphs = useWorkspaceGraphs(adapter, connectionId, sources);
  const crons = useCronJobs(cronWorkspace);
  const [selection, setSelection] = useState<Selection>(initialBlueprintId ? { kind: "blueprint", id: initialBlueprintId } : initialScenarioId ? { kind: "scenario", id: initialScenarioId } : initialCronId ? { kind: "cron", id: initialCronId } : null);
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
  const pipelines = useMemo(() => graphs.flatMap((graph) => {
    if (!graph.graph) return [];
    const explicit = graph.graph.blueprints ?? [];
    const entries = explicit.map((blueprint) => ({ workspaceId: graph.attachment.workspace_id, pipeline: pipelineModel(blueprint) }));
    const known = new Set(explicit.map((blueprint) => blueprint.id));
    const legacy = graph.graph.scenarios
      .filter((scenario) => !known.has(scenario.id))
      .map((scenario) => ({ workspaceId: graph.attachment.workspace_id, pipeline: legacyScenarioPipeline(graph.graph!, scenario) }));
    return [...entries, ...legacy].filter(({ pipeline }) => filter.teamId === null || pipeline.owner.team_id === filter.teamId);
  }), [filter.teamId, graphs]);
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
  const openedBlueprint = selection?.kind === "blueprint" ? pipelines.find(({ pipeline }) => pipeline.id === selection.id) : undefined;
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

  const actionContext = useMemo<JobActionContext>(() => ({ adapter, jobActions: jobActionAdapter, graphFor, refreshCrons: crons.refresh }), [adapter, jobActionAdapter, graphFor, crons.refresh]);
  const deleteJob = useCallback((job: MergedJob) => deleteMergedJob(job, actionContext), [actionContext]);

  const stopJob = useCallback(async (job: MergedJob) => {
    const key = jobKey(job);
    setStoppingJob(key);
    setActionErrors((current) => ({ ...current, [key]: "" }));
    try {
      await stopMergedJob(job, actionContext);
    } catch {
      setActionErrors((current) => ({ ...current, [key]: `Could not stop ${job.title}.` }));
    } finally {
      setStoppingJob(null);
    }
  }, [actionContext]);

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
        ? deleteJobConfirmation(confirmation.job).error
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
  if (selection?.kind === "blueprint") {
    return <section className="min-w-0 p-4" aria-label="Blueprint view">
      <button type="button" className={AUTOMATION_TOOL} onClick={() => back(`blueprint:${selection.id}`)}>← All jobs</button>
      {openedBlueprint
        ? <div className="mt-3 space-y-3"><h3 className="text-[length:var(--ct-font-size-md)] font-semibold">{openedBlueprint.pipeline.name}</h3><BlueprintPipelineCanvas pipeline={openedBlueprint.pipeline} /></div>
        : graphsRead ? <p role="alert" className="mt-3">The selected Blueprint is unavailable.</p> : <p role="status" className="mt-3">Reading Blueprint…</p>}
    </section>;
  }
  if (opened) {
    const { graph, scenario } = opened;
    return <ScenarioPanel state={graph} scope={graph.attachment} scenario={scenario} camera={cameraFor(scenario.id)} backLabel="All jobs" headingLevel={3} fill onBack={() => back(`scenario:${scenario.id}`)}>
      <AutomationNotices state={graph} noun="Blueprints" />
    </ScenarioPanel>;
  }
  const reading = (crons.jobs === null && !crons.failed) || graphs.some((graph) => graph.loading);
  const empty = jobs.length === 0 && pipelines.length === 0 && crons.jobs !== null && graphsRead && !crons.failed && !graphsFailed;
  return <div ref={list} className="mx-auto flex w-full max-w-3xl flex-1 flex-col px-4 py-4 sm:px-6">
    {missing && <p ref={removed} tabIndex={-1} role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2 outline-none">The selected Blueprint is unavailable. It may have been removed; choose another job.</p>}
    {crons.failed && <p role="alert" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2">Cron jobs unavailable. {crons.jobs ? "Showing the last list read." : "No cron records could be read."}</p>}
    <AutomationNotices state={notices} noun="Blueprints" />
    {reading && <p role="status" className="mb-3 text-[var(--ct-muted-fg)]">Reading jobs…</p>}
    <div role="status" aria-label={empty ? "No jobs" : undefined} className={empty ? "flex flex-1 flex-col items-center justify-center gap-1.5 py-10 text-center" : undefined}>
      {empty && <EmptyJobs scope={emptyScope} canWiden={canWiden} />}
    </div>
    {!empty && <ul aria-label="Jobs" className="flex flex-col gap-2">
      {jobs.map((job: MergedJob) => {
        const key = jobKey(job);
        const canStop = canStopJob(job);
        const actionItems = [
          ...(canStop ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stoppingJob === key, onClick: () => void stopJob(job) }] : []),
          { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => beginDelete(job) },
        ];
        const onTriggerRef = (element: HTMLButtonElement | null) => { if (element) menuTriggers.current.set(key, element); else menuTriggers.current.delete(key); };
        return job.kind === "cron"
          ? <JobRow key={key} jobKey={key} icon={<Clock3 size={15} aria-hidden="true" />} name={job.title} meta={cronMeta(job.cron)} type="Cron" state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "cron", id: job.cron.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />
          : <JobRow key={key} jobKey={key} icon={job.sentinel.type === "Blueprint graph" ? <Network size={15} aria-hidden="true" /> : <Radar size={15} aria-hidden="true" />} name={job.title} meta={sentinelMeta(job.sentinel)} detail={unnamedGraphId(job.sentinel, job.title)} type={job.sentinel.type} state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "scenario", id: job.sentinel.scenario.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />;
      })}
    </ul>}
    {pipelines.length > 0 && <section aria-label="Blueprints by owner" className="mt-6 space-y-4">
      {groupBlueprintsByOwner(pipelines.map(({ pipeline }) => pipeline)).map((group) => {
        const ownerName = group.kind === "team"
          ? teams.find((team) => team.id === group.ownerId)?.name?.trim() || group.ownerId || "Team"
          : group.kind === "tab"
            ? tabs[group.ownerId ?? ""]?.title || group.ownerId || "Tab"
            : "Unbound owner";
        return <section key={group.key} aria-label={`${group.kind === "team" ? "Team" : "Tab"} ${ownerName}`} className="space-y-2">
          <h3 className="flex items-center gap-2 text-sm font-semibold"><span>{ownerName}</span><span className="rounded-full border border-[var(--ct-border)] px-2 py-0.5 text-[0.625rem] font-normal text-[var(--ct-muted-fg)]">{group.kind === "team" ? "Team" : group.kind === "tab" ? "Tab" : "Owner unavailable"}</span></h3>
          {pipelines.filter(({ pipeline }) => group.blueprints.some((entry) => entry.id === pipeline.id)).map(({ workspaceId, pipeline }) => <article key={`${workspaceId}:${pipeline.id}`} className="space-y-2 rounded-lg border border-[var(--ct-border)] p-3">
            <button type="button" data-job-key={`blueprint:${pipeline.id}`} className="text-left font-medium hover:text-[var(--ct-accent)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" aria-label={`Open Blueprint ${pipeline.name}, ${pipeline.state}`} onClick={() => setSelection({ kind: "blueprint", id: pipeline.id })}>{pipeline.name} · {pipeline.state}</button>
            <BlueprintPipelineCanvas pipeline={pipeline} />
          </article>)}
        </section>;
      })}
    </section>}
    {confirmation && <JobConfirmationDialog
      title={confirmation.kind === "delete" ? deleteJobConfirmation(confirmation.job).title : "Clear stopped jobs?"}
      description={confirmation.kind === "delete"
        ? deleteJobConfirmation(confirmation.job).description
        : `Delete ${confirmation.jobs.length} stopped or disabled jobs in the current filter, including their history? This cannot be undone.`}
      confirmLabel={confirmation.kind === "delete" ? deleteJobConfirmation(confirmation.job).confirmLabel : "Clear stopped"}
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
  tabs?: Record<string, Tab>;
  initialFilter?: JobsFilter;
  initialScenarioId?: string;
  initialCronId?: string;
  initialBlueprintId?: string;
  onClose?: () => void;
  adapter?: AutomationGraphAdapter;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

/** The one Jobs view: crons and Blueprint graphs narrowed by optional workspace and Team capsules. Hosts differ only by initial filter. */
export function JobsView({ workspaces, teams, tabs, initialFilter = NO_JOBS_FILTER, initialScenarioId, initialCronId, initialBlueprintId, onClose, adapter = hostAutomationAdapter, cronActionAdapter, jobActionAdapter = hostJobsActionAdapter }: JobsViewProps) {
  const context = usePlanPeerContext();
  const [filter, setFilter] = useState<JobsFilter>(initialFilter);
  const [initialScenario, setInitialScenario] = useState(initialScenarioId);
  const [initialCron, setInitialCron] = useState(initialCronId);
  const [initialBlueprint, setInitialBlueprint] = useState(initialBlueprintId);
  const clearStoppedButton = useRef<HTMLButtonElement>(null);
  const [clearState, setClearState] = useState<{ count: number; run: (() => void) | null }>({ count: 0, run: null });
  const setClearStopped = useCallback((count: number, run: (() => void) | null) => setClearState({ count, run }), []);
  const changeFilter = useCallback((next: JobsFilter) => { setInitialScenario(undefined); setInitialCron(undefined); setInitialBlueprint(undefined); setFilter(next); }, []);
  const layout = useMemo(() => jobsLayout(filter, workspaces, teams), [filter, workspaces, teams]);
  const sourcesKey = JSON.stringify(layout.sources);
  const sources = useMemo<string[]>(() => JSON.parse(sourcesKey), [sourcesKey]);
  const connectionId = `host:${context.ready ? context.generation : "detached"}`;
  return <section aria-label="Jobs" className="flex h-full min-h-0 flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
    <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-titlebar-bg)_82%,transparent)] px-4 py-3 backdrop-blur-md sm:px-6">
      <Workflow size={18} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
      <h2 className="text-[length:var(--ct-font-size-md)] font-semibold">Jobs</h2>
      <JobsFilterBar filter={filter} layout={layout} teams={teams} onChange={changeFilter} />
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <button ref={clearStoppedButton} type="button" className={`${AUTOMATION_TOOL} ct-liquid-interactive`} aria-label={`Clear stopped${clearState.count ? `, ${clearState.count} jobs` : ""}`} title="Clear stopped" disabled={!clearState.run} onClick={() => clearState.run?.()}>
          Clear stopped{clearState.count > 0 && <span className="rounded-full border border-[var(--ct-border)] px-1.5 py-px text-[0.6875rem]">{clearState.count}</span>}
        </button>
        {onClose && <button type="button" className={`${AUTOMATION_TOOL} ct-liquid-interactive`} aria-label="Close jobs" title="Close jobs" onClick={onClose}><X size={15} aria-hidden="true" /></button>}
      </div>
    </header>
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      {!context.ready ? <p role="status" className="p-4">Attaching jobs…</p>
        : !layout.teamAvailable ? <p role="alert" className="p-4">The selected Team is unavailable.</p>
          : <JobsSurface key={JSON.stringify([connectionId, sources, filter])} adapter={adapter} connectionId={connectionId} sources={sources} cronWorkspace={layout.cronWorkspace} filter={filter} emptyScope={layout.emptyScope} canWiden={layout.canWiden} initialScenarioId={initialScenario} initialCronId={initialCron} initialBlueprintId={initialBlueprint} teams={teams} tabs={tabs} cronActionAdapter={cronActionAdapter} jobActionAdapter={jobActionAdapter} onClearStoppedChange={setClearStopped} clearStoppedButtonRef={clearStoppedButton} />}
    </div>
  </section>;
}
