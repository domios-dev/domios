import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Clock3, Network, Radar, Square, Trash2, Workflow } from "lucide-react";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { sentinelJobs } from "../Automation/scenarios";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";
import { deleteJobConfirmation, deleteMergedJob, hostJobsActionAdapter, jobWorkspace, stopMergedJob, type JobActionContext, type JobsActionAdapter } from "../Jobs/jobActions";
import { canStopJob, jobStateLabel, jobsForTab, mergeJobs, NO_JOBS_FILTER, tabJobAccessibleName, tabJobKey, mergedJobId, type MergedJob, type TabJob } from "../Jobs/jobsModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { pipelineModel } from "../Automation/pipelineModel";
import { useCronJobs } from "../Jobs/useCronJobs";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";

const VISIBLE_JOBS = 3;
const NO_SOURCES: readonly string[] = [];

function JobIcon({ job }: { job: MergedJob }) {
  const Icon = job.kind === "cron" ? Clock3 : job.sentinel.type === "Blueprint graph" ? Network : Radar;
  return <Icon size={12} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />;
}

function stateTone(chip: TabJob): string {
  if (chip.state === "active") return "text-[var(--ct-accent)]";
  return chip.state === "waiting on you" ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]";
}

function openJob(job: MergedJob) {
  requestOpenJob({
    workspaceId: jobWorkspace(job),
    target: job.kind === "cron" ? { teamId: null, cronId: job.cron.id } : { teamId: null, scenarioId: job.sentinel.scenario.id },
  });
}

export interface ComposerJobChipsProps {
  tabId: string;
  workspaceId: string | null;
  adapter?: AutomationGraphAdapter;
  jobActionAdapter?: JobsActionAdapter;
}

/** Jobs bound to this tab, read from the same Cron list and Sentinel graph sources as the Jobs view. */
export function ComposerJobChips({ tabId, workspaceId, adapter = hostAutomationAdapter, jobActionAdapter = hostJobsActionAdapter }: ComposerJobChipsProps) {
  const sources = useMemo(() => workspaceId ? [workspaceId] : NO_SOURCES, [workspaceId]);
  const graphs = useWorkspaceGraphs(adapter, "composer", sources);
  const crons = useCronJobs(null);
  const sentinels = useMemo(() => graphs.flatMap((graph) => graph.graph ? sentinelJobs(graph.graph, graph.attachment) : []), [graphs]);
  const blueprints = useMemo(() => graphs.flatMap((graph) => (graph.graph?.blueprints ?? [])
    .filter((blueprint) => blueprint.owner?.tab_id === tabId)
    .map((blueprint) => ({ workspaceId: graph.attachment.workspace_id, blueprint, pipeline: pipelineModel(blueprint) }))), [graphs, tabId]);
  const merged = useMemo(() => mergeJobs(NO_JOBS_FILTER, crons.jobs ?? [], sentinels), [crons.jobs, sentinels]);
  const chips = useMemo(() => jobsForTab(merged, tabId), [merged, tabId]);
  const byKey = useMemo(() => new Map(merged.map((job) => [tabJobKey({ kind: job.kind, id: mergedJobId(job) }), job])), [merged]);
  const graphFor = useCallback((id: string) => graphs.find((graph) => graph.attachment.workspace_id === id), [graphs]);
  const context = useMemo<JobActionContext>(() => ({ adapter, jobActions: jobActionAdapter, graphFor, refreshCrons: crons.refresh }), [adapter, jobActionAdapter, graphFor, crons.refresh]);
  const triggers = useRef(new Map<string, HTMLButtonElement>());
  const returnFocus = useRef<HTMLElement | null>(null);
  const [confirming, setConfirming] = useState<MergedJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stopping, setStopping] = useState<string | null>(null);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const overflowRef = useRef<HTMLDivElement>(null);
  const overflowPopoverId = `composer-more-jobs-${useId().replace(/:/g, "")}`;

  useEffect(() => {
    if (!overflowOpen) return;
    const closeOnOutside = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && !overflowRef.current?.contains(target) && !(target instanceof Element && target.closest('[role="menu"]'))) {
        setOverflowOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !document.querySelector('[role="menu"]')) setOverflowOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [overflowOpen]);

  useLayoutEffect(() => {
    if (confirming || !returnFocus.current) return;
    if (returnFocus.current.isConnected) returnFocus.current.focus();
    returnFocus.current = null;
  }, [confirming]);

  const stop = async (key: string, job: MergedJob) => {
    setStopping(key);
    try {
      await stopMergedJob(job, context);
    } catch {
      setError(`Could not stop ${job.title}.`);
    } finally {
      setStopping(null);
    }
  };

  const confirmDelete = async () => {
    if (!confirming) return;
    setBusy(true);
    setError(null);
    try {
      await deleteMergedJob(confirming, context);
    } catch {
      setBusy(false);
      setError(deleteJobConfirmation(confirming).error);
      return;
    }
    await Promise.allSettled([crons.refresh(), ...graphs.map((graph) => graph.refresh())]);
    setBusy(false);
    setConfirming(null);
  };

  if (chips.length === 0 && blueprints.length === 0 && !confirming) return null;
  const visible = chips.slice(0, VISIBLE_JOBS);
  const hidden = chips.slice(VISIBLE_JOBS);
  const copy = confirming ? deleteJobConfirmation(confirming) : null;
  const renderChip = (chip: TabJob, inOverflow = false) => {
    const key = tabJobKey(chip);
    const job = byKey.get(key);
    if (!job) return null;
    const actionsLabel = `Actions for ${chip.title}`;
    const items = [
      ...(canStopJob(job) ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stopping === key, onClick: () => void stop(key, job) }] : []),
      { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => { returnFocus.current = triggers.current.get(key) ?? null; setError(null); setConfirming(job); } },
    ];
    return <div key={key} data-job-key={key} data-job-state={jobStateLabel(chip.state)} className={`ct-composer-activity-chip h-7 min-w-0 max-w-[16rem]${inOverflow ? " w-full" : ""}`}>
      <button type="button" aria-label={tabJobAccessibleName(chip)} title={`${chip.title}, ${jobStateLabel(chip.state)}`} className="ct-composer-activity-chip__open" onMouseDown={(event) => event.preventDefault()} onClick={() => openJob(job)}>
        <JobIcon job={job} />
        <span className="ct-composer-activity-chip__primary">{chip.title}</span>
        <span className={`shrink-0 ${stateTone(chip)}`}>{jobStateLabel(chip.state)}</span>
      </button>
      <DropdownMenu
        menuLabel={actionsLabel}
        items={items}
        className="flex shrink-0 items-center"
        trigger={<RowMenuButton ref={(element) => { if (element) triggers.current.set(key, element); else triggers.current.delete(key); }} label={actionsLabel} />}
      />
    </div>;
  };
  return <div role="group" aria-label="Jobs for this tab" className="contents">
    {visible.map((chip) => renderChip(chip))}
    {hidden.length > 0 && <div ref={overflowRef} className="relative">
      <button type="button" aria-label={`${hidden.length} more jobs`} aria-expanded={overflowOpen} aria-controls={overflowPopoverId} className="ct-composer-activity-chip h-7 tabular-nums text-[var(--ct-muted-fg)]" onClick={() => setOverflowOpen((open) => !open)}>+{hidden.length}</button>
      {overflowOpen && <div id={overflowPopoverId} role="group" aria-label="More jobs for this tab" className="absolute bottom-full right-0 z-50 mb-1 flex max-h-64 min-w-64 flex-col gap-1 overflow-y-auto rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg-elev,var(--ct-surface))] p-2 shadow-xl">
        {hidden.map((chip) => renderChip(chip, true))}
      </div>}
    </div>}
    {blueprints.map(({ workspaceId, blueprint, pipeline }) => <button
      key={`blueprint:${blueprint.id}`}
      type="button"
      data-blueprint-id={blueprint.id}
      data-job-state={jobStateLabel(blueprint.state)}
      aria-label={`Blueprint ${blueprint.name}, ${jobStateLabel(blueprint.state)}`}
      title={`${blueprint.name}, ${jobStateLabel(blueprint.state)}`}
      className="ct-composer-activity-chip h-7 min-w-0 max-w-[16rem]"
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => requestOpenJob({ workspaceId, target: { teamId: blueprint.owner?.team_id ?? null, blueprintId: pipeline.id } })}
    >
      <Workflow size={12} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
      <span className="ct-composer-activity-chip__primary">{blueprint.name}</span>
      <span className={`shrink-0 ${jobStateLabel(blueprint.state) === "active" ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted-fg)]"}`}>{jobStateLabel(blueprint.state)}</span>
    </button>)}
    {error && !confirming && <span role="alert" className="text-xs text-[var(--ct-fg)]">{error}</span>}
    {confirming && copy && <JobConfirmationDialog
      title={copy.title}
      description={copy.description}
      confirmLabel={copy.confirmLabel}
      busy={busy}
      error={error}
      onCancel={() => { if (!busy) { setConfirming(null); setError(null); } }}
      onConfirm={() => void confirmDelete()}
    />}
  </div>;
}
