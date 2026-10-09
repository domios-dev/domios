import { useEffect, useState } from "react";
import type { CronJob, CronRunInfo } from "../../lib/types";
import type { InventoryOperation, Service } from "./types";
import { formatTime } from "../../lib/formatters";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { CronPurposeCard } from "./CronDrawerSection/CronPurposeCard";
import { CronRunHistory } from "./CronDrawerSection/CronRunHistory";
import { CronScheduleCard } from "./CronDrawerSection/CronScheduleCard";
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
  const running = ["running", "awaitingownerprompt"].includes(service.last_run_status?.toLowerCase() ?? "");

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

  return (
    <div className="ct-job-detail" data-testid="mesh-cron-actions" aria-busy={busy}>
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-resource-announcement">{announcement}</div>
      <CronScheduleCard
        service={service}
        busy={busy}
        running={running}
        editing={editingSchedule}
        scheduleInput={scheduleInput}
        toggleOperation={toggleOperation}
        can={can}
        reason={reason}
        onScheduleInput={setScheduleInput}
        onEdit={() => setEditingSchedule(true)}
        onCancelEdit={() => { setEditingSchedule(false); setScheduleInput(service.schedule ?? ""); }}
        onSave={() => void handleSaveSchedule()}
        onTrigger={() => void handleTrigger()}
        onToggle={() => void handleToggle()}
        onCancelRun={() => void handleCancel()}
      />
      {job && <CronPurposeCard job={job} />}
      <CronRunHistory runs={runs} allowed={can("history")} reason={reason("history")} onOpen={setOpenRun} />
      {openRun && openRun.session_id && <RunChatPopup title={`cron:${service.name}`} subtitle={formatTime(openRun.started_at)} status={openRun.status} provider={job?.agent ?? ""} cwd={openRun.cwd ?? ""} sessionId={openRun.session_id} live={openRun.status === "Running" || openRun.status === "AwaitingOwnerPrompt"} onClose={() => setOpenRun(null)} />}
    </div>
  );
}
