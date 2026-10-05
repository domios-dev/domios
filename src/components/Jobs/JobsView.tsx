import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { ChevronRight, Clock3, MoreHorizontal, Network, Radar, Square, Trash2 } from "lucide-react";
import { invoke } from "../../lib/bridge";
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
import { AUTOMATION_ROW, AUTOMATION_TOOL, stateTone } from "../Automation/styles";
import type { AutomationAttachment, AutomationGraphAdapter } from "../Automation/types";
import { useAutomationGraph } from "../Automation/useAutomationGraph";
import { DropdownMenu } from "../shared/DropdownMenu";
import { JobConfirmationDialog } from "./JobConfirmationDialog";
import { useCronJobs } from "./useCronJobs";
import { ALL_JOBS, clearableJobs, cronState, mergeJobs, type JobsTeamFilter, type MergedJob } from "./jobsModel";

export interface JobsTarget {
  teamId: JobsTeamFilter;
  scenarioId: string;
}

type Selection = { kind: "cron" | "scenario"; id: string } | null;

interface JobsViewProps {
  workspaceId: string;
  teamId: JobsTeamFilter;
  adapter?: AutomationGraphAdapter;
  initialScenarioId?: string;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter?: JobsActionAdapter;
  onClearStoppedChange?: (count: number, onClear: (() => void) | null) => void;
  clearStoppedButtonRef?: RefObject<HTMLButtonElement | null>;
}

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

function JobRow({ jobKey: key, icon, name, meta, detail, type, state, actionItems, onOpen, onTriggerRef, error }: {
  jobKey: string;
  icon: ReactNode;
  name: string;
  meta: string;
  detail?: string;
  type: string;
  state: string;
  actionItems: Array<{ label: string; icon?: ReactNode; disabled?: boolean; danger?: boolean; onClick: () => void }>;
  onOpen: () => void;
  onTriggerRef: (element: HTMLButtonElement | null) => void;
  error: string | null;
}) {
  const actionsLabel = `Actions for ${name}`;
  return <li className="min-w-0">
    <div className="flex min-w-0 items-stretch gap-1">
      <button type="button" data-job-key={key} aria-label={`${name}, ${type}, ${state}`} className={`${AUTOMATION_ROW} flex-1`} onClick={onOpen}>
        <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-[var(--ct-titlebar-bg)] text-[var(--ct-muted-fg)]">{icon}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[length:var(--ct-font-size-sm)] font-medium">{name}</span>
          <span className="block truncate text-xs text-[var(--ct-muted-fg)]">{meta}{detail && <span className="text-[var(--ct-muted)]"> · {detail}</span>}</span>
        </span>
        <span className="flex shrink-0 flex-col items-end gap-1">
          <span className="rounded-full border border-[var(--ct-border)] px-2 py-px text-[0.6875rem] text-[var(--ct-muted-fg)]">{type}</span>
          <span className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]"><span className={`h-1.5 w-1.5 rounded-full ${stateTone(state)}`} aria-hidden="true" />{state}</span>
        </span>
        <ChevronRight size={14} className="shrink-0 text-[var(--ct-muted)]" aria-hidden="true" />
      </button>
      <DropdownMenu
        menuLabel={actionsLabel}
        items={actionItems}
        className="flex shrink-0 items-center"
        trigger={<button ref={onTriggerRef} type="button" className={`${AUTOMATION_TOOL} h-full min-w-9 px-2`} aria-label={actionsLabel} aria-haspopup="menu" title={actionsLabel}><MoreHorizontal size={16} aria-hidden="true" /></button>}
      />
    </div>
    {error && <p role="alert" className="px-3 pt-1 text-xs text-[var(--ct-fg)]">{error}</p>}
  </li>;
}

function cronMeta(job: CronJob): string {
  const schedule = describeSchedule(job.schedule);
  return job.enabled && job.next_run ? `${schedule} · next ${formatFuture(job.next_run)}` : schedule;
}

function unnamedGraphId(job: SentinelJob, title: string): string | undefined {
  const { name, id } = job.scenario;
  return (!name.trim() || name === id) && title !== id ? id : undefined;
}

function sentinelMeta(job: SentinelJob): string {
  return job.scenario.name === job.scenario.id ? job.scenario.root.id : job.root?.purpose ?? job.scenario.root.id;
}

function JobsSurface({ adapter, attachment, initialScenarioId, cronActionAdapter, jobActionAdapter, onClearStoppedChange, clearStoppedButtonRef }: {
  adapter: AutomationGraphAdapter;
  attachment: AutomationAttachment;
  initialScenarioId?: string;
  cronActionAdapter?: MeshResourceActionAdapter;
  jobActionAdapter: JobsActionAdapter;
  onClearStoppedChange?: JobsViewProps["onClearStoppedChange"];
  clearStoppedButtonRef?: JobsViewProps["clearStoppedButtonRef"];
}) {
  const automation = useAutomationGraph(adapter, attachment);
  const crons = useCronJobs(attachment.workspace_id);
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
  const scopedCrons = useMemo(() => crons.jobs?.filter((job) => job.workspace_id === attachment.workspace_id && (attachment.team_id === ALL_JOBS || (job.team_id ?? null) === attachment.team_id)) ?? null, [crons.jobs, attachment.workspace_id, attachment.team_id]);
  const sentinels = useMemo(() => automation.graph ? sentinelJobs(automation.graph, attachment) : null, [automation.graph, attachment.workspace_id, attachment.team_id]);
  const jobs = useMemo(() => mergeJobs(attachment.workspace_id, attachment.team_id, scopedCrons ?? [], sentinels ?? []), [attachment.workspace_id, attachment.team_id, scopedCrons, sentinels]);
  const clearable = useMemo(() => clearableJobs(jobs), [jobs]);
  const canClear = automation.graph !== null && crons.jobs !== null && !automation.failed && !crons.failed;
  const scenario = selection?.kind === "scenario" ? automation.graph?.scenarios.find((entry) => entry.id === selection.id) : undefined;
  const missing = selection?.kind === "scenario" && automation.graph !== null && !scenario;
  const back = (key: string) => { returnTo.current = key; setSelection(null); };

  const requestClearStopped = useCallback(() => {
    if (!canClear || clearable.length === 0) return;
    dialogFocus.current = clearStoppedButtonRef?.current ?? null;
    setConfirmationError(null);
    setConfirmation({ kind: "clear", jobs: clearable });
  }, [canClear, clearable, clearStoppedButtonRef]);

  useEffect(() => {
    onClearStoppedChange?.(canClear ? clearable.length : 0, canClear && clearable.length > 0 ? requestClearStopped : null);
    return () => onClearStoppedChange?.(0, null);
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
    const identity = job.sentinel.scenario.root;
    return adapter.delete(attachment, identity);
  }, [adapter, attachment, jobActionAdapter]);

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
        if (!root) return;
        await adapter.stop(attachment, root.identity);
        await automation.refresh();
      }
    } catch {
      setActionErrors((current) => ({ ...current, [key]: `Could not stop ${job.title}.` }));
    } finally {
      setStoppingJob(null);
    }
  }, [adapter, attachment, automation.refresh, crons.refresh, jobActionAdapter]);

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
    await Promise.all([crons.refresh(), automation.refresh()]);
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
      {jobs.map((job: MergedJob) => {
        const key = jobKey(job);
        const canStop = job.kind === "cron"
          ? cronState(job.cron) === "Running"
          : ["active", "running", "finishing"].includes(job.state.toLowerCase()) && Boolean(job.sentinel.root?.stop.allowed);
        const actionItems = [
          ...(canStop ? [{ label: "Stop", icon: <Square size={12} aria-hidden="true" />, disabled: stoppingJob === key, onClick: () => void stopJob(job) }] : []),
          { label: "Delete", icon: <Trash2 size={12} aria-hidden="true" />, danger: true, onClick: () => beginDelete(job) },
        ];
        return job.kind === "cron"
          ? <JobRow key={key} jobKey={key} icon={<Clock3 size={15} aria-hidden="true" />} name={job.title} meta={`${cronMeta(job.cron)} · last run: ${job.cron.last_run?.status ?? "none"}`} type="Cron" state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "cron", id: job.cron.id })} onTriggerRef={(element) => { if (element) menuTriggers.current.set(key, element); else menuTriggers.current.delete(key); }} error={actionErrors[key] || null} />
          : <JobRow key={key} jobKey={key} icon={job.sentinel.type === "Sentinel graph" ? <Network size={15} aria-hidden="true" /> : <Radar size={15} aria-hidden="true" />} name={job.title} meta={sentinelMeta(job.sentinel)} detail={unnamedGraphId(job.sentinel, job.title)} type={job.sentinel.type} state={job.state} actionItems={actionItems} onOpen={() => setSelection({ kind: "scenario", id: job.sentinel.scenario.id })} onTriggerRef={(element) => { if (element) menuTriggers.current.set(key, element); else menuTriggers.current.delete(key); }} error={actionErrors[key] || null} />;
      })}
    </ul>
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

export function JobsView({ workspaceId, teamId, adapter = hostAutomationAdapter, initialScenarioId, cronActionAdapter, jobActionAdapter = hostJobsActionAdapter, onClearStoppedChange, clearStoppedButtonRef }: JobsViewProps) {
  const context = usePlanPeerContext();
  if (!context.ready) return <p role="status" className="p-4">Attaching jobs…</p>;
  const attachment = { workspace_id: workspaceId, team_id: teamId, connection_id: `host:${context.generation}` };
  return <JobsSurface key={JSON.stringify([attachment.connection_id, workspaceId, teamId])} adapter={adapter} attachment={attachment} initialScenarioId={initialScenarioId} cronActionAdapter={cronActionAdapter} jobActionAdapter={jobActionAdapter} onClearStoppedChange={onClearStoppedChange} clearStoppedButtonRef={clearStoppedButtonRef} />;
}
