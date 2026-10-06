import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Clock3, Network, Radar, Square, Trash2 } from "lucide-react";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { sentinelJobs } from "../Automation/scenarios";
import type { AutomationGraphAdapter } from "../Automation/types";
import { JobConfirmationDialog } from "../Jobs/JobConfirmationDialog";
import { deleteJobConfirmation, deleteMergedJob, hostJobsActionAdapter, jobWorkspace, stopMergedJob, type JobActionContext, type JobsActionAdapter } from "../Jobs/jobActions";
import { canStopJob, jobsForTab, mergeJobs, NO_JOBS_FILTER, tabJobAccessibleName, tabJobKey, mergedJobId, type MergedJob, type TabJob } from "../Jobs/jobsModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { useCronJobs } from "../Jobs/useCronJobs";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";
import { DropdownMenu } from "../shared/DropdownMenu";
import { RowMenuButton } from "../shared/RowMenuButton";

const VISIBLE_JOBS = 3;
const NO_SOURCES: readonly string[] = [];

function JobIcon({ job }: { job: MergedJob }) {
  const Icon = job.kind === "cron" ? Clock3 : job.sentinel.type === "Sentinel graph" ? Network : Radar;
  return <Icon size={12} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />;
}

function stateTone(chip: TabJob): string {
  if (chip.state === "running") return "text-[var(--ct-accent)]";
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

  if (chips.length === 0 && !confirming) return null;
  const visible = chips.slice(0, VISIBLE_JOBS);
  const hidden = chips.slice(VISIBLE_JOBS);
  const copy = confirming ? deleteJobConfirmation(confirming) : null;
  return <div role="group" aria-label="Jobs for this tab" className="contents">
    {visible.map((chip) => {
      const key = tabJobKey(chip);
      const job = byKey.get(key);
      if (!job) return null;
      const actionsLabel = `Actions for ${chip.title}`;
      const items = [
        ...(canStopJob(job) ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stopping === key, onClick: () => void stop(key, job) }] : []),
        { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => { returnFocus.current = triggers.current.get(key) ?? null; setError(null); setConfirming(job); } },
      ];
      return <div key={key} data-job-key={key} className="ct-composer-activity-chip h-7 min-w-0 max-w-[16rem]">
        <button type="button" aria-label={tabJobAccessibleName(chip)} title={`${chip.title}, ${chip.state}`} className="ct-composer-activity-chip__open" onMouseDown={(event) => event.preventDefault()} onClick={() => openJob(job)}>
          <JobIcon job={job} />
          <span className="ct-composer-activity-chip__primary">{chip.title}</span>
          <span className={`shrink-0 ${stateTone(chip)}`}>{chip.state}</span>
        </button>
        <DropdownMenu
          menuLabel={actionsLabel}
          items={items}
          className="flex shrink-0 items-center"
          trigger={<RowMenuButton ref={(element) => { if (element) triggers.current.set(key, element); else triggers.current.delete(key); }} label={actionsLabel} />}
        />
      </div>;
    })}
    {hidden.length > 0 && <DropdownMenu
      menuLabel="More jobs for this tab"
      items={hidden.flatMap((chip) => {
        const job = byKey.get(tabJobKey(chip));
        return job ? [{ label: chip.title, description: chip.state, onClick: () => openJob(job) }] : [];
      })}
      className="flex shrink-0 items-center"
      trigger={<button type="button" aria-label={`${hidden.length} more jobs`} aria-haspopup="menu" className="ct-composer-activity-chip h-7 tabular-nums text-[var(--ct-muted-fg)]">+{hidden.length}</button>}
    />}
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
