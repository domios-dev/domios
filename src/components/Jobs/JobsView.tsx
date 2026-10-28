import { SavedSolutions } from "./SavedSolutions";
import { useJobsFilterNavigation, useJobsNavigation, useMissingJobFallback } from "../../hooks/useJobsNavigation";
import { SavedSolutionView } from "./SavedSolutionView";
import { SaveSolutionForm } from "./SaveSolutionForm";
import { successfulSolutionScope, blueprintSolutionsApi, type BlueprintSolutionsApi, type SolutionScope } from "./blueprintSolutions";
import { blueprintMeta } from "../Automation/blueprintMeta";
import type { TransportClient } from "../../lib/transportClient";
import { JobsMachineFilter } from "./JobsMachineFilter";
import { PeerJobsView } from "./PeerJobsView";
import { jobsEntryState, THIS_MACHINE } from "./jobsMachineModel";
import { useJobsMachines } from "./useJobsMachines";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Clock3, Network, Plus, Square, Trash2, Workflow } from "lucide-react";
import { BackPill } from "../shared/BackPill";
import { returnToOrigin, type ReturnTo, type ReturnToRoute } from "../../lib/returnTo";
import type { PairedPeer, Tab, Team } from "../../lib/types";
import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { CronWorkspaceView } from "../Cron/CronWorkspaceView";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { AutomationNotices } from "../Automation/AutomationGraphView";
import { focusEntry, focusLost } from "../Automation/focus";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { HostBlueprintView } from "../Automation/blueprint/HostBlueprintView";
import { blueprintScenarioJobs } from "../Automation/scenarios";
import { AUTOMATION_TOOL } from "../Automation/styles";
import type { AutomationGraphAdapter } from "../Automation/types";
import { BlueprintImportForm } from "./BlueprintImportForm";
import { JobConfirmationDialog } from "./JobConfirmationDialog";
import { cronMeta, JobRow, sentinelRowText, sentinelStateLabel } from "./JobRow";
import { JobsFilterBar } from "./JobsFilterBar";
import { jobsLayout, type JobsEmptyScope, type JobsWorkspace } from "./jobsLayout";
import { useCronJobs } from "./useCronJobs";
import { useWorkspaceGraphs } from "./useWorkspaceGraphs";
import { deleteJobConfirmation, deleteMergedJob, hostJobsActionAdapter, stopMergedJob, type JobActionContext, type JobsActionAdapter } from "./jobActions";
import { blueprintJobs, canStopJob, clearableJobs, jobStateLabel, mergeJobs, NO_JOBS_FILTER, type JobsFilter, type MergedJob } from "./jobsModel";

export type { JobsActionAdapter } from "./jobActions";

export interface JobsTarget {
  teamId: string | null;
  scenarioId?: string;
  runId?: string;
  cronId?: string;
  blueprintId?: string;
}

type Confirmation = { kind: "delete"; job: MergedJob } | { kind: "clear"; jobs: MergedJob[] };

function jobKey(job: MergedJob): string {
  return job.kind === "blueprint" ? `blueprint:${job.blueprint.pipeline.id}` : job.kind === "cron" ? `cron:${job.cron.id}` : `scenario:${job.sentinel.scenario.id}`;
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
  solutionsApi: BlueprintSolutionsApi;
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
  initialRunId?: string;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter: JobsActionAdapter;
  onClearStoppedChange: (count: number, onClear: (() => void) | null) => void;
  clearStoppedButtonRef: RefObject<HTMLButtonElement | null>;
  /** Closes the whole Jobs surface, returning to whatever opened it (a chat card or chip). */
  onExit?: () => void;
  onReturn?: () => boolean;
  onDetailChange: (open: boolean) => void;
  workspaceName: (id: string) => string;
}

function JobsSurface({ solutionsApi, adapter, connectionId, sources, cronWorkspace, filter, emptyScope, canWiden, initialScenarioId, initialCronId, initialBlueprintId, initialRunId, cronActionAdapter, jobActionAdapter, onClearStoppedChange, clearStoppedButtonRef, onExit, onReturn, onDetailChange, workspaceName }: SurfaceProps) {
  const graphs = useWorkspaceGraphs(adapter, connectionId, sources);
  const [saveScope, setSaveScope] = useState<SolutionScope | null>(null);
  const [solutionRevision, setSolutionRevision] = useState(0);
  const [solutionSaved, setSolutionSaved] = useState(false);
  const crons = useCronJobs(cronWorkspace);
  const { selection, setSelection, openedSolution, setOpenedSolution } = useJobsNavigation(JSON.stringify(filter), initialBlueprintId ? { kind: "blueprint", id: initialBlueprintId } : initialScenarioId ? { kind: "scenario", id: initialScenarioId } : initialCronId ? { kind: "cron", id: initialCronId } : null, solutionsApi);
  useEffect(() => {
    onDetailChange(selection !== null || openedSolution !== null);
    return () => onDetailChange(false);
  }, [onDetailChange, selection, openedSolution]);
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
  const sentinels = useMemo(() => graphs.flatMap((graph) => graph.graph ? blueprintScenarioJobs(graph.graph, graph.attachment) : []), [graphs]);
  const blueprints = useMemo(() => graphs.flatMap((graph) => graph.graph ? blueprintJobs(graph.graph) : []), [graphs]);
  const jobs = useMemo(() => mergeJobs(filter, crons.jobs ?? [], sentinels, blueprints), [filter, crons.jobs, sentinels, blueprints]);
  const pipelines = useMemo(() => graphs.flatMap((graph) => {
    if (!graph.graph) return [];
    return blueprintJobs(graph.graph)
      .map(({ pipeline }) => ({ workspaceId: graph.attachment.workspace_id, pipeline }))
      .filter(({ pipeline }) => filter.teamId === null || pipeline.owner.team_id === filter.teamId);
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
  useMissingJobFallback(selection, selection?.kind === "cron" ? crons.jobs !== null && !crons.failed : selection?.kind !== "scenario" && graphsRead && !graphsFailed,
    selection?.kind === "cron" ? Boolean(crons.jobs?.some((job) => job.id === selection.id)) : Boolean(opened || openedBlueprint), setSelection);
  const missing = selection?.kind === "scenario" && graphsRead && !opened;
  const notices = useMemo(() => ({ graph: graphs.find((graph) => graph.graph?.truncated)?.graph ?? graphs.find((graph) => graph.graph)?.graph ?? null, loading: false, failed: graphsFailed, stopping: null, stopFailed: null }), [graphs, graphsFailed]);
  const refreshAll = useCallback(() => Promise.all([crons.refresh(), ...graphs.map((graph) => graph.refresh())]), [crons.refresh, graphs]);
  const back = (key: string) => { if (onReturn?.()) return; returnTo.current = key; setSelection(null); };
  const openedDirectly = useRef(initialScenarioId ?? null);
  const closeScenario = (id: string) => {
    if (onReturn?.()) return;
    if (openedDirectly.current === id && onExit) return onExit();
    openedDirectly.current = null;
    back(`scenario:${id}`);
  };

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
    if (target?.isConnected && !(target instanceof HTMLButtonElement && target.disabled)) target.focus();
    else {
      const fallback = list.current?.querySelector<HTMLButtonElement>("button[data-job-key]") ?? clearStoppedButtonRef.current?.closest<HTMLElement>('[aria-label="Jobs"]');
      fallback?.focus();
    }
  }, [confirmation, jobs, clearStoppedButtonRef]);

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
    if ((selection && !missing) || openedSolution) return;
    if (returnTo.current) focusEntry(list.current, "jobKey", returnTo.current);
    else if (missing && focusLost()) removed.current?.focus();
    returnTo.current = null;
  }, [selection, missing, openedSolution]);

  if (openedSolution) {
    const id = openedSolution.solution.id;
    const close = () => { returnTo.current = `solution:${id}`; setOpenedSolution(null); };
    return <SavedSolutionView key={id} entry={openedSolution} api={solutionsApi} backLabel="All jobs" onBack={close}
      onDeleted={() => { setOpenedSolution(null); setSolutionRevision((revision) => revision + 1); }} />;
  }
  if (selection?.kind === "cron") {
    const id = selection.id;
    return <CronWorkspaceView cronId={id} actionAdapter={cronActionAdapter} backLabel="All jobs" onBack={() => { void crons.refresh(); back(`cron:${id}`); }} />;
  }
  if (selection?.kind === "blueprint" || opened) {
    const target = opened ? { graph: opened.graph, id: opened.scenario.root.id } : openedBlueprint ? { graph: graphFor(openedBlueprint.workspaceId), id: openedBlueprint.pipeline.id } : null;
    const scenarioId = opened?.scenario.id ?? null;
    const backLabel = scenarioId && openedDirectly.current === scenarioId && onExit ? "where you were" : "All jobs";
    const onBack = () => scenarioId ? closeScenario(scenarioId) : back(`${blueprints.some(({ pipeline }) => pipeline.id === selection?.id) ? "blueprint" : "scenario"}:${selection?.id}`);
    if (target?.graph?.graph) return <HostBlueprintView graph={target.graph.graph} attachment={target.graph.attachment} adapter={adapter} refresh={target.graph.refresh}
      blueprintId={target.id} initialRunId={!scenarioId || initialScenarioId === scenarioId ? initialRunId : undefined} backLabel={backLabel} onBack={onBack} headingLevel={3} fill />;
    return <section className="flex min-w-0 flex-col gap-3 px-4 py-3" aria-label="Blueprint view">
      <BackPill label={backLabel} onBack={onBack} />
      {graphsRead ? <InlineErrorNotice error={{ code: "blueprint_unavailable", raw: "" }} /> : graphsFailed ? <p role="status" className="ct-job-note">Blueprint is unavailable.</p> : <p role="status" className="ct-job-note">Reading Blueprint…</p>}
    </section>;
  }
  const reading = (crons.jobs === null && !crons.failed) || graphs.some((graph) => graph.loading);
  const empty = jobs.length === 0 && crons.jobs !== null && graphsRead && !crons.failed && !graphsFailed;
  return <div ref={list} className="mx-auto flex w-full max-w-3xl flex-1 flex-col px-4 py-4 sm:px-6">
    {saveScope && <SaveSolutionForm api={solutionsApi} scope={saveScope} onCancel={() => setSaveScope(null)} onSaved={() => { setSaveScope(null); setSolutionSaved(true); setSolutionRevision((revision) => revision + 1); }} />}
    {solutionSaved && <p role="status">Solution saved.</p>}
    {missing && <div ref={removed} tabIndex={-1} role="status" className="mb-3 rounded-md border border-[var(--ct-border)] px-3 py-2 outline-none"><InlineErrorNotice error={{ code: "blueprint_unavailable", raw: "" }} /></div>}
    {crons.failed && crons.jobs === null && <p role="status" aria-label="Jobs unavailable" className="ct-job-note">Jobs are unavailable.</p>}
    <AutomationNotices state={notices} noun="Blueprints" />
    {reading && <p role="status" className="mb-3 text-[var(--ct-muted-fg)]">Reading jobs…</p>}
    <div role="status" aria-label={empty ? "No jobs" : undefined} className={empty ? "flex flex-1 flex-col items-center justify-center gap-1.5 py-10 text-center" : undefined}>
      {empty && <EmptyJobs scope={emptyScope} canWiden={canWiden} />}
    </div>
    {!empty && <ul aria-label="Jobs" className="flex flex-col gap-2">
      {jobs.map((job: MergedJob) => {
        const key = jobKey(job);
        const canStop = canStopJob(job);
        const solutionScope = successfulSolutionScope(job);
        const actionItems = [
          ...(solutionScope ? [{ label: "Save as solution", onClick: () => { setSolutionSaved(false); setSaveScope(solutionScope); } }] : []),
          ...(canStop ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stoppingJob === key, onClick: () => void stopJob(job) }] : []),
          ...(job.kind === "cron" && job.cron.protected ? [] : [{ label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => beginDelete(job) }]),
        ];
        const onTriggerRef = (element: HTMLButtonElement | null) => { if (element) menuTriggers.current.set(key, element); else menuTriggers.current.delete(key); };
        if (job.kind === "blueprint") return <JobRow key={key} jobKey={key} icon={<Network size={15} aria-hidden="true" />} name={job.title} meta={blueprintMeta(job.blueprint.pipeline.note || job.blueprint.pipeline.nodes[0]?.summary || "", job.state, job.blueprint.pipeline.deletesAt)} type="Blueprint" tone="blueprint" heldReason={job.blueprint.pipeline.heldReason} state={jobStateLabel(job.state)} actionItems={actionItems} onOpen={() => setSelection({ kind: "blueprint", id: job.blueprint.pipeline.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />;
        return job.kind === "cron"
          ? <JobRow key={key} jobKey={key} icon={<Clock3 size={15} aria-hidden="true" />} name={job.title} meta={cronMeta(job.cron)} type={job.cron.role === "curator" ? "Curator" : "Cron"} tone="cron" state={jobStateLabel(job.state)} actionItems={actionItems} onOpen={() => setSelection({ kind: "cron", id: job.cron.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />
          : <JobRow key={key} jobKey={key} icon={<Network size={15} aria-hidden="true" />} name={job.title} {...sentinelRowText(job.sentinel, job.title)} type={job.sentinel.type} tone="blueprint" heldReason={job.sentinel.root?.held_reason} state={sentinelStateLabel(job.sentinel)} actionItems={actionItems} onOpen={() => setSelection({ kind: "scenario", id: job.sentinel.scenario.id })} onTriggerRef={onTriggerRef} error={actionErrors[key] || null} />;
      })}
    </ul>}
    {/* An empty scope shows only its empty state; saved solutions join a list that has jobs. */}
    {!empty && <SavedSolutions api={solutionsApi} sources={sources} revision={solutionRevision} workspaceName={workspaceName} onOpen={setOpenedSolution} />}
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
  solutionsApi?: BlueprintSolutionsApi;
  machinePeers?: readonly PairedPeer[];
  peerClient?: (peerId: string) => TransportClient;
  workspaces: readonly JobsWorkspace[];
  teams: readonly Team[];
  tabs?: Record<string, Tab>;
  initialFilter?: JobsFilter;
  initialScenarioId?: string;
  initialCronId?: string;
  initialBlueprintId?: string;
  initialRunId?: string;
  returnTo?: ReturnTo;
  onReturnTo?: (origin: ReturnTo) => boolean;
  onClose?: () => void;
  adapter?: AutomationGraphAdapter;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

/** The one Jobs view: crons and Blueprint graphs narrowed by optional workspace and Team capsules. Hosts differ only by initial filter. */
export function JobsView({ solutionsApi = blueprintSolutionsApi, machinePeers, peerClient, workspaces, teams, tabs = {}, initialFilter = NO_JOBS_FILTER, initialScenarioId, initialCronId, initialBlueprintId, initialRunId, returnTo, onReturnTo, onClose, adapter = hostAutomationAdapter, cronActionAdapter, jobActionAdapter = hostJobsActionAdapter }: JobsViewProps) {
  const origin = useRef<ReturnToRoute>({ returnTo });
  const onReturn = () => {
    if (!returnToOrigin(origin.current, onReturnTo ?? (() => false))) return false;
    onClose?.();
    return true;
  };
  const close = () => { if (!onReturn()) onClose?.(); };
  const context = usePlanPeerContext();
  const machines = useJobsMachines(machinePeers === undefined);
  const peers = machinePeers ?? machines.peers;
  const [machineId, setMachineId] = useState(jobsEntryState({ kind: "machine" }).machineId);
  const [importOpen, setImportOpen] = useState(false);
  const workspaceName = (id: string) => workspaces.find((workspace) => workspace.id === id)?.name ?? id;
  const [detailOpen, setDetailOpen] = useState(false);
  const [importRevision, setImportRevision] = useState(0);
  const [filter, setFilter] = useJobsFilterNavigation(initialFilter, !initialScenarioId && !initialCronId && !initialBlueprintId, context.ready, workspaces);
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
  const changeMachine = (id: string) => { changeFilter(NO_JOBS_FILTER); setMachineId(id); };
  if (machineId !== THIS_MACHINE) return <PeerJobsView clientForPeer={peerClient} key={machineId} peerId={machineId} peers={peers} onMachineChange={changeMachine} onClose={close} />;
  return <section aria-label="Jobs" tabIndex={-1} className="flex h-full min-h-0 flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]">
    <header hidden={detailOpen} className="ct-jobs-header flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 sm:px-6">
      {onClose && <BackPill label="where you were" onBack={close} />}
      <h2 className="flex items-center gap-2 text-[length:var(--ct-font-size-md)] font-semibold"><Workflow size={16} className="shrink-0 text-[var(--ct-tone-blueprint)]" aria-hidden="true" />Jobs</h2>
      <JobsMachineFilter value={machineId} peers={peers} onChange={changeMachine} />
      <JobsFilterBar filter={filter} layout={layout} teams={teams} onChange={changeFilter} />
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <button type="button" className={AUTOMATION_TOOL} aria-expanded={importOpen} onClick={() => setImportOpen(true)}><Plus size={12} aria-hidden="true" />Import Blueprint</button>
        <button ref={clearStoppedButton} type="button" className={AUTOMATION_TOOL} aria-label={`Clear stopped${clearState.count ? `, ${clearState.count} jobs` : ""}`} title="Clear stopped" disabled={!clearState.run} onClick={() => clearState.run?.()}>
          Clear stopped{clearState.count > 0 && <span className="text-[var(--ct-fg)]">{clearState.count}</span>}
        </button>
      </div>
    </header>
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      {machines.failed && machinePeers === undefined && <p role="status" className="px-4 py-2">Paired machine list unavailable.</p>}
      {importOpen && <div className="mx-auto w-full max-w-3xl px-4 pt-4 sm:px-6"><BlueprintImportForm creators={Object.values(tabs).filter((tab) => tab.team_id && teams.some((team) => team.id === tab.team_id && (filter.teamId === null || team.id === filter.teamId) && (filter.workspaceId === null || team.workspace_id === filter.workspaceId))).map((tab) => ({ id: tab.id, label: tab.title, teamId: tab.team_id ?? undefined }))} teams={teams} workspaces={workspaces} workspaceIds={sources} workspaceName={workspaceName} onCancel={() => setImportOpen(false)} onCreated={() => { setImportOpen(false); setImportRevision((revision) => revision + 1); }} /></div>}
      {importRevision > 0 && <p role="status" className="px-4 py-2">Blueprint imported stopped.</p>}
      {!context.ready ? <p role="status" className="p-4">Attaching jobs…</p>
        : !layout.teamAvailable ? <InlineErrorNotice error={{ code: "team_unavailable", raw: "" }} />
          : <JobsSurface solutionsApi={solutionsApi} key={JSON.stringify([connectionId, sources, filter, importRevision])} adapter={adapter} connectionId={connectionId} sources={sources} cronWorkspace={layout.cronWorkspace} filter={filter} emptyScope={layout.emptyScope} canWiden={layout.canWiden} initialScenarioId={initialScenario} initialCronId={initialCron} initialBlueprintId={initialBlueprint} initialRunId={initialRunId} cronActionAdapter={cronActionAdapter} jobActionAdapter={jobActionAdapter} onClearStoppedChange={setClearStopped} clearStoppedButtonRef={clearStoppedButton} onExit={onClose} onReturn={onReturn} onDetailChange={setDetailOpen} workspaceName={workspaceName} />}
    </div>
  </section>;
}
