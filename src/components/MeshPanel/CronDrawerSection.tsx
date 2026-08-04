import { invoke } from "../../lib/bridge";
import { useEffect, useState } from "react";
import type { CronJob, CronRunInfo } from "../../lib/types";
import { serviceIdToCronId } from "./types";
import { describeSchedule, formatFuture, formatTime } from "../../lib/formatters";
import type { Service } from "./types";
import { UI_BUTTON, UI_INPUT, UI_PANEL } from "../shared/design";
import { DropdownSelect, type DropdownSelectOption } from "../shared/DropdownMenu";
import { Card } from "../shared/Card";
import { Chip } from "../shared/Chip";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { useAgentProviders } from "../../hooks/useAgentProviders";

interface Props {
  service: Service;
  onUpdate: () => void;
}

export function CronDrawerSection({ service, onUpdate }: Props) {
  const [editingSchedule, setEditingSchedule] = useState(false);
  const [scheduleInput, setScheduleInput] = useState(service.schedule ?? "");
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [agent, setAgent] = useState<string>("claude");
  const [job, setJob] = useState<CronJob | null>(null);
  const [openRun, setOpenRun] = useState<CronRunInfo | null>(null);
  const { providers } = useAgentProviders();
  const cronId = serviceIdToCronId(service.id);
  const sourcePeerId = service.source_peer_id;
  const isRemote = !!sourcePeerId;
  const actionsDisabled = busy || !!service.stale;
  const scheduleInputClass = `${UI_INPUT.full} flex-1 font-mono outline-none border border-[var(--ct-accent)] text-[10px]`;
  const scheduleBtnClass = `${UI_BUTTON.compact} ${UI_BUTTON.base} border-[var(--ct-border)] text-[var(--ct-muted)]`;
  const runsMetaClass = "text-[var(--ct-muted-fg)] font-mono";

  useEffect(() => {
    let cancelled = false;
    invoke<CronRunInfo[]>(
      isRemote ? "mesh_peer_cron_history" : "cron_history",
      isRemote ? { instanceId: sourcePeerId, id: cronId } : { id: cronId },
    )
      .then((r) => {
        if (!cancelled) setRuns(Array.isArray(r) ? r.slice().reverse() : []);
      })
      .catch(() => {});
    invoke<CronJob>(isRemote ? "mesh_peer_cron_get" : "cron_get", isRemote
      ? { instanceId: sourcePeerId, id: cronId }
      : { id: cronId })
      .then((next) => {
        if (cancelled) return;
        setJob(next);
        if (next?.agent) setAgent(next.agent);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [cronId, isRemote, sourcePeerId]);

  const handleTrigger = async () => {
    setBusy(true);
    try {
      await invoke(
        isRemote ? "mesh_peer_cron_action" : "cron_trigger",
        isRemote
          ? { instanceId: sourcePeerId, id: cronId, action: "trigger" }
          : { id: cronId },
      );
      onUpdate();
    } catch (e) {
      alert(`Trigger failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleToggle = async () => {
    setBusy(true);
    try {
      await invoke(
        isRemote ? "mesh_peer_cron_action" : "cron_toggle",
        isRemote
          ? {
              instanceId: sourcePeerId,
              id: cronId,
              action: service.enabled ? "disable" : "enable",
            }
          : { id: cronId, enabled: !service.enabled },
      );
      onUpdate();
    } catch (e) {
      alert(`Toggle failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleSaveSchedule = async () => {
    if (!scheduleInput.trim()) return;
    setBusy(true);
    try {
      await invoke("cron_update", { id: cronId, schedule: scheduleInput.trim() });
      setEditingSchedule(false);
      onUpdate();
    } catch (e) {
      alert(`Update failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleAgentChange = async (agentId: string) => {
    if (!job || isRemote) return;
    const provider = providers.find((item) => item.id === agentId);
    setBusy(true);
    try {
      const next = await invoke<CronJob>("cron_update", {
        id: cronId,
        agent: agentId,
        model: provider?.default_model ?? "",
      });
      setJob(next);
      setAgent(next.agent);
      onUpdate();
    } finally {
      setBusy(false);
    }
  };

  const handleModelChange = async (model: string) => {
    if (!job || isRemote) return;
    setBusy(true);
    try {
      const next = await invoke<CronJob>("cron_update", { id: cronId, model });
      setJob(next);
      onUpdate();
    } finally {
      setBusy(false);
    }
  };

  const providerOptions: DropdownSelectOption[] = providers.map((provider) => ({
    value: provider.id,
    label: provider.display_name,
  }));
  const selectedProvider = job ? providers.find((provider) => provider.id === job.agent) : undefined;
  const modelOptions: DropdownSelectOption[] = [
    { value: "", label: "Default model" },
    ...(selectedProvider?.available_models ?? []).map((model) => ({
      value: model.id,
      label: model.display_name,
    })),
  ];

  return (
    <div className="flex flex-col gap-3">
      {job && (
        <Section label="Purpose">
          <div className="whitespace-pre-wrap break-words text-xs leading-relaxed text-[var(--ct-fg)]">
            {job.description?.trim() || job.task}
          </div>
          {job.description?.trim() && (
            <div className="mt-2 border-t border-[var(--ct-border)] pt-2">
              <div className="mb-1 text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">Agent task</div>
              <div className="whitespace-pre-wrap break-words text-[10px] text-[var(--ct-muted-fg)]">{job.task}</div>
            </div>
          )}
          <div className="mt-3 flex flex-wrap gap-1.5 text-[10px]">
            <MetaPill>{job.report_target_tab_id ? `Reports to ${job.report_target_tab_id}` : "Private worker"}</MetaPill>
            <MetaPill>Action · Start agent</MetaPill>
            <MetaPill>{job.oneshot ? "Runs once" : "Recurring"}</MetaPill>
            <MetaPill>{job.cwd ?? "Workspace directory"}</MetaPill>
          </div>
          <div className="mt-3 grid grid-cols-2 gap-2">
            <div>
              <div className="mb-1 text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">Agent provider</div>
              <DropdownSelect
                options={providerOptions}
                value={job.agent}
                onChange={(value) => { void handleAgentChange(value); }}
                disabled={actionsDisabled || isRemote || providerOptions.length === 0}
              />
            </div>
            <div>
              <div className="mb-1 text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">Model</div>
              <DropdownSelect
                options={modelOptions}
                value={job.model ?? ""}
                onChange={(value) => { void handleModelChange(value); }}
                disabled={actionsDisabled || isRemote}
              />
            </div>
          </div>
        </Section>
      )}
      <Section label="Schedule">
        {editingSchedule ? (
          <div className="flex gap-1.5">
            <input
              value={scheduleInput}
              onChange={(e) => setScheduleInput(e.target.value)}
              className={scheduleInputClass}
              placeholder="*/30 * * * *"
            />
            <button
              onClick={handleSaveSchedule}
              disabled={actionsDisabled}
              className={`${UI_BUTTON.primary} ${UI_BUTTON.compact} disabled:opacity-50`}
            >
              Save
            </button>
            <button
              onClick={() => { setEditingSchedule(false); setScheduleInput(service.schedule ?? ""); }}
              className={scheduleBtnClass}
            >
              ×
            </button>
          </div>
        ) : (
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className={`h-1.5 w-1.5 rounded-full ${service.enabled ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
                <span className="text-xs text-[var(--ct-fg)]">
                  {service.schedule ? describeSchedule(service.schedule) : "Schedule not set"}
                </span>
              </div>
              <div className="mt-1 pl-3.5 font-mono text-[10px] text-[var(--ct-muted)]">
                {service.schedule ?? "—"}
              </div>
              <div className="mt-2 pl-3.5 text-[10px] text-[var(--ct-muted-fg)]">
                Next run {service.next_run ? `${formatFuture(service.next_run)} · ${formatTime(service.next_run)}` : "not scheduled"}
                {service.last_run_status ? ` · Last ${service.last_run_status.toLowerCase()}` : ""}
              </div>
            </div>
            <button
              onClick={() => setEditingSchedule(true)}
              disabled={isRemote || !!service.stale}
              className={`${UI_BUTTON.base} ${UI_BUTTON.compact} text-[var(--ct-muted)] border-none`}
              title={
                service.stale
                  ? "Cached cron state is not editable"
                  : isRemote
                    ? "Remote cron schedule editing is not wired yet"
                    : "Edit schedule"
              }
            >
              Edit
            </button>
          </div>
        )}
      </Section>

      <Section label="Controls">
        <div className="flex gap-1.5">
          <button
            onClick={handleTrigger}
            disabled={actionsDisabled}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} flex-1 disabled:opacity-50`}
            title={service.stale ? "Cached cron state is not actionable" : undefined}
          >
            ▶ Trigger now
          </button>
          <button
            onClick={handleToggle}
            disabled={actionsDisabled}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} flex-1 disabled:opacity-50`}
            title={service.stale ? "Cached cron state is not actionable" : undefined}
          >
            {service.enabled ? "⏸ Pause" : "▶ Resume"}
          </button>
        </div>
      </Section>

      <Section label="Run history">
        {runs.length === 0 ? (
          <div className="text-[10px] text-[var(--ct-muted)]">No runs yet</div>
        ) : (
          <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
            {runs.map((run) => {
              const inspectable = !!run.session_id;
              return (
                <button
                  key={run.run_id}
                  type="button"
                  disabled={!inspectable}
                  onClick={() => inspectable && setOpenRun(run)}
                  title={inspectable ? "View run transcript" : "No transcript for this run"}
                  className={`flex items-center gap-2 text-[10px] bg-transparent border-0 p-0 text-left ${inspectable ? "cursor-pointer hover:brightness-125" : "cursor-default opacity-80"}`}
                >
                  <span className={runColor(run.status)}>{runIcon(run.status)}</span>
                  <span className="text-[var(--ct-muted)] flex-1">{formatTime(run.started_at)}</span>
                  {run.finished_at && (
                    <span className={runsMetaClass}>
                      {Math.round((run.finished_at - run.started_at) / 1000)}s
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </Section>

      {openRun && openRun.session_id && (
        <RunChatPopup
          title={`cron:${service.name}`}
          subtitle={formatTime(openRun.started_at)}
          status={openRun.status}
          provider={agent}
          cwd={openRun.cwd ?? ""}
          sessionId={openRun.session_id}
          live={openRun.status === "Running"}
          onClose={() => setOpenRun(null)}
        />
      )}
    </div>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className={UI_PANEL.sectionLabel}>{label}</div>
      {children}
    </Card>
  );
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
