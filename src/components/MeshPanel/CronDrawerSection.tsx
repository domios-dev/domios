import { useEffect, useState } from "react";
import type { CronJob, CronRunInfo } from "../../lib/types";
import type { InventoryOperation, Service } from "./types";
import { describeSchedule, formatFuture, formatTime } from "../../lib/formatters";
import { Card } from "../shared/Card";
import { Chip } from "../shared/Chip";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { UI_BUTTON, UI_INPUT, UI_PANEL } from "../shared/design";
import {
  capabilityFor,
  capabilityReason,
  createMeshResourceActionAdapter,
  mutationFailure,
  resourceTarget,
  unavailableFailure,
  type MeshResourceActionAdapter,
} from "./meshResourceActions";

interface Props {
  service: Service;
  machineName: string;
  onUpdate: () => void;
  actionAdapter?: MeshResourceActionAdapter;
  onAnnouncement?: (message: string) => void;
}

const defaultAdapter = createMeshResourceActionAdapter();

export function CronDrawerSection({ service, machineName, onUpdate, actionAdapter = defaultAdapter, onAnnouncement }: Props) {
  const [editingSchedule, setEditingSchedule] = useState(false);
  const [scheduleInput, setScheduleInput] = useState(service.schedule ?? "");
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [job, setJob] = useState<CronJob | null>(null);
  const [openRun, setOpenRun] = useState<CronRunInfo | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const target = resourceTarget(service, machineName);
  const can = (operation: InventoryOperation) => capabilityFor(service, operation).allowed;
  const reason = (operation: InventoryOperation) => capabilityReason(service, operation);
  const scheduleInputClass = `${UI_INPUT.full} flex-1 font-mono outline-none border border-[var(--ct-accent)] text-[10px]`;
  const scheduleBtnClass = `${UI_BUTTON.compact} ${UI_BUTTON.base} border-[var(--ct-border)] text-[var(--ct-muted)]`;
  const runsMetaClass = "text-[var(--ct-muted-fg)] font-mono";
  const running = service.last_run_status?.toLowerCase() === "running";

  useEffect(() => {
    setScheduleInput(service.schedule ?? "");
  }, [service.id, service.schedule]);

  useEffect(() => {
    let cancelled = false;
    if (can("history")) {
      void actionAdapter.history(service)
        .then((history) => {
          if (!cancelled) setRuns(Array.isArray(history) ? history.slice().reverse() : []);
        })
        .catch(() => undefined);
    } else {
      setRuns([]);
    }
    if (can("get")) {
      void actionAdapter.get(service)
        .then((next) => {
          if (!cancelled) setJob(next);
        })
        .catch(() => undefined);
    } else {
      setJob(null);
    }
    return () => {
      cancelled = true;
    };
  }, [actionAdapter, service.id, service.capabilities, service.schedule]);

  const confirmAction = (message: string): boolean =>
    typeof window === "undefined" || typeof window.confirm !== "function" || window.confirm(message);

  const runMutation = async (
    operation: InventoryOperation,
    label: string,
    action: () => Promise<unknown>,
    confirmation?: string,
  ) => {
    const capability = capabilityFor(service, operation);
    if (!capability.allowed) {
      const message = unavailableFailure(label, capability.unavailable_reason);
      setAnnouncement(message);
      onAnnouncement?.(message);
      return false;
    }
    if (confirmation && !confirmAction(confirmation)) return false;
    setBusy(true);
    try {
      await action();
      const message = `${label} succeeded on target machine ${target.name}.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onUpdate();
      return true;
    } catch (error) {
      const message = mutationFailure(label, target, error);
      setAnnouncement(message);
      onAnnouncement?.(message);
      return false;
    } finally {
      setBusy(false);
    }
  };

  const handleTrigger = () => runMutation(
    "trigger",
    "Trigger",
    () => actionAdapter.trigger(service),
    target.remote ? `Run schedule "${service.name}" on target machine "${target.name}"?` : undefined,
  );
  const toggleOperation: InventoryOperation = service.enabled ? "disable" : "enable";
  const handleToggle = () => runMutation(
    toggleOperation,
    service.enabled ? "Pause" : "Resume",
    () => actionAdapter.setEnabled(service, !service.enabled),
    target.remote ? `${service.enabled ? "Pause" : "Resume"} schedule "${service.name}" on target machine "${target.name}"?` : undefined,
  );
  const handleCancel = () => runMutation(
    "cancel",
    "Cancel running execution",
    () => actionAdapter.cancel(service),
    `Cancel the running execution of schedule "${service.name}" on target machine "${target.name}"?`,
  );
  const handleSaveSchedule = async () => {
    const schedule = scheduleInput.trim();
    if (!schedule) return;
    const succeeded = await runMutation(
      "update_schedule",
      "Update schedule",
      () => actionAdapter.updateSchedule(service, schedule),
      target.remote ? `Update schedule "${service.name}" on target machine "${target.name}"?` : undefined,
    );
    if (succeeded) setEditingSchedule(false);
  };
  const actionDisabled = busy || !can("update_schedule");

  return (
    <div className="flex flex-col gap-3" data-testid="mesh-cron-actions" aria-busy={busy}>
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-resource-announcement">{announcement}</div>
      {job && (
        <Section label="Purpose">
          <div className="whitespace-pre-wrap break-words text-xs leading-relaxed text-[var(--ct-fg)]">{job.description?.trim() || job.task}</div>
          {job.description?.trim() && <div className="mt-2 border-t border-[var(--ct-border)] pt-2"><div className="mb-1 text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">Agent task</div><div className="whitespace-pre-wrap break-words text-[10px] text-[var(--ct-muted-fg)]">{job.task}</div></div>}
          <div className="mt-3 flex flex-wrap gap-1.5 text-[10px]"><MetaPill>{job.report_target_tab_id ? `Reports to ${job.report_target_tab_id}` : "Private worker"}</MetaPill><MetaPill>Action · Start agent</MetaPill><MetaPill>{job.oneshot ? "Runs once" : "Recurring"}</MetaPill><MetaPill>{job.cwd ?? "Workspace directory"}</MetaPill></div>
          <div className="mt-3 grid grid-cols-2 gap-2 text-[10px] text-[var(--ct-muted-fg)]"><div>Agent provider <span className="font-mono text-[var(--ct-fg)]">{job.agent}</span></div><div>Model <span className="font-mono text-[var(--ct-fg)]">{job.model ?? "default"}</span></div></div>
        </Section>
      )}
      <Section label="Schedule">
        {editingSchedule ? (
          <div className="flex gap-1.5"><input value={scheduleInput} onChange={(event) => setScheduleInput(event.target.value)} className={scheduleInputClass} placeholder="*/30 * * * *" aria-label="Schedule expression" /><button type="button" onClick={() => void handleSaveSchedule()} disabled={actionDisabled} className={`${UI_BUTTON.primary} ${UI_BUTTON.compact} disabled:opacity-50`}>Save</button><button type="button" onClick={() => { setEditingSchedule(false); setScheduleInput(service.schedule ?? ""); }} className={scheduleBtnClass} aria-label="Cancel schedule edit">×</button></div>
        ) : (
          <div className="flex items-start gap-2"><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className={`h-1.5 w-1.5 rounded-full ${service.enabled ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} /><span className="text-xs text-[var(--ct-fg)]">{service.schedule ? describeSchedule(service.schedule) : "Schedule not set"}</span></div><div className="mt-1 pl-3.5 font-mono text-[10px] text-[var(--ct-muted)]">{service.schedule ?? "—"}</div><div className="mt-2 pl-3.5 text-[10px] text-[var(--ct-muted-fg)]">Next run {service.next_run ? `${formatFuture(service.next_run)} · ${formatTime(service.next_run)}` : "not scheduled"}{service.last_run_status ? ` · Last ${service.last_run_status.toLowerCase()}` : ""}</div></div><button type="button" onClick={() => setEditingSchedule(true)} disabled={busy || !can("update_schedule")} className={`${UI_BUTTON.base} ${UI_BUTTON.compact} text-[var(--ct-muted)] border-none disabled:opacity-50`} title={reason("update_schedule") ?? "Edit schedule"}>Edit</button></div>
        )}
        {!can("update_schedule") && <Unavailable operation="update_schedule" reason={reason("update_schedule")} />}
      </Section>

      <Section label="Controls">
        <div className="flex flex-wrap gap-1.5"><ActionButton label="▶ Trigger now" allowed={can("trigger")} busy={busy} unavailableReason={reason("trigger")} onClick={() => void handleTrigger()} /><ActionButton label={service.enabled ? "⏸ Pause" : "▶ Resume"} allowed={can(toggleOperation)} busy={busy} unavailableReason={reason(toggleOperation)} onClick={() => void handleToggle()} />{running && <ActionButton label="■ Cancel running" allowed={can("cancel")} busy={busy} unavailableReason={reason("cancel")} onClick={() => void handleCancel()} />}</div>
        {!can("trigger") && <Unavailable operation="trigger" reason={reason("trigger")} />}
        {!can(toggleOperation) && <Unavailable operation={toggleOperation} reason={reason(toggleOperation)} />}
        {running && !can("cancel") && <Unavailable operation="cancel" reason={reason("cancel")} />}
      </Section>

      <Section label="Run history">
        {can("history") ? runs.length === 0 ? <div className="text-[10px] text-[var(--ct-muted)]">No runs yet</div> : <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">{runs.map((run) => { const inspectable = !!run.session_id; return <button key={run.run_id} type="button" disabled={!inspectable} onClick={() => inspectable && setOpenRun(run)} title={inspectable ? "View run transcript" : "No transcript for this run"} className={`flex items-center gap-2 border-0 bg-transparent p-0 text-left text-[10px] ${inspectable ? "cursor-pointer hover:brightness-125" : "cursor-default opacity-80"}`}><span className={runColor(run.status)}>{runIcon(run.status)}</span><span className="flex-1 text-[var(--ct-muted)]">{formatTime(run.started_at)}</span>{run.finished_at && <span className={runsMetaClass}>{Math.round((run.finished_at - run.started_at) / 1000)}s</span>}</button>; })}</div> : <Unavailable operation="history" reason={reason("history")} />}
      </Section>
      {openRun && openRun.session_id && <RunChatPopup title={`cron:${service.name}`} subtitle={formatTime(openRun.started_at)} status={openRun.status} provider={job?.agent ?? ""} cwd={openRun.cwd ?? ""} sessionId={openRun.session_id} live={openRun.status === "Running"} onClose={() => setOpenRun(null)} />}
    </div>
  );
}

function ActionButton({ label, allowed, busy, unavailableReason, onClick }: { label: string; allowed: boolean; busy: boolean; unavailableReason?: string; onClick: () => void }) {
  return <button type="button" onClick={onClick} disabled={busy || !allowed} className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} flex-1 disabled:opacity-50`} title={unavailableReason}>{label}</button>;
}

function Unavailable({ operation, reason }: { operation: InventoryOperation; reason?: string }) {
  return <div className="text-[10px] text-[var(--ct-yellow)]" data-testid={`mesh-operation-unavailable-${operation}`}>Unavailable{reason ? `: ${reason}` : ""}</div>;
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return <Card className="flex flex-col gap-2 p-4"><div className={UI_PANEL.sectionLabel}>{label}</div>{children}</Card>;
}

function MetaPill({ children }: { children: React.ReactNode }) {
  return <Chip variant="muted">{children}</Chip>;
}

function runIcon(status: string): string {
  if (status === "Running") return "●";
  if (status === "Success") return "✓";
  if (status === "Cancelled") return "■";
  return "✗";
}

function runColor(status: string): string {
  if (status === "Running") return "text-[var(--ct-accent)] animate-pulse";
  if (status === "Success") return "text-[var(--ct-green)]";
  if (status === "Cancelled") return "text-[var(--ct-muted-fg)]";
  return "text-[var(--ct-red)]";
}
