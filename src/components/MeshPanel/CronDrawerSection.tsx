import { invoke } from "../../lib/bridge";
import { useEffect, useState } from "react";
import type { CronJob, CronRunInfo } from "../../lib/types";
import { serviceIdToCronId } from "./types";
import { describeSchedule, formatFuture, formatTime } from "../../lib/formatters";
import type { Service } from "./types";
import { UI_BUTTON, UI_INPUT, UI_PANEL } from "../shared/design";
import { RunChatPopup } from "../AgentRun/RunChatPopup";

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
  const [openRun, setOpenRun] = useState<CronRunInfo | null>(null);
  const cronId = serviceIdToCronId(service.id);
  const scheduleInputClass = `${UI_INPUT.full} flex-1 font-mono outline-none border border-[var(--ct-accent)] text-[10px]`;
  const scheduleBtnClass = `${UI_BUTTON.compact} ${UI_BUTTON.base} border-[var(--ct-border)] text-[var(--ct-muted)]`;
  const sectionMetaClass = "text-[10px] text-[var(--ct-fg)] flex flex-col gap-0.5";
  const runsMetaClass = "text-[var(--ct-muted-fg)] font-mono";

  useEffect(() => {
    let cancelled = false;
    invoke<CronRunInfo[]>("cron_history", { id: cronId })
      .then((r) => {
        if (!cancelled) setRuns(Array.isArray(r) ? r.slice(-5).reverse() : []);
      })
      .catch(() => {});
    invoke<CronJob>("cron_get", { id: cronId })
      .then((j) => {
        if (!cancelled && j?.agent) setAgent(j.agent);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [cronId]);

  const handleTrigger = async () => {
    setBusy(true);
    try {
      await invoke("cron_trigger", { id: cronId });
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
      await invoke("cron_toggle", { id: cronId, enabled: !service.enabled });
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

  return (
    <>
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
              disabled={busy}
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
          <div className="flex items-center gap-2">
            <span className="font-mono text-[10px] text-[var(--ct-fg)] flex-1">
              {service.schedule ? describeSchedule(service.schedule) : "(unset)"}
              {service.schedule && (
                <span className="text-[var(--ct-muted)] ml-1">({service.schedule})</span>
              )}
            </span>
            <button
              onClick={() => setEditingSchedule(true)}
              className={`${UI_BUTTON.base} ${UI_BUTTON.compact} text-[var(--ct-muted)] border-none`}
            >
              Edit
            </button>
          </div>
        )}
      </Section>

      <Section label="Cron actions">
        <div className="flex gap-1.5">
          <button
            onClick={handleTrigger}
            disabled={busy}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} flex-1 disabled:opacity-50`}
          >
            ▶ Trigger now
          </button>
          <button
            onClick={handleToggle}
            disabled={busy}
            className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact} flex-1 disabled:opacity-50`}
          >
            {service.enabled ? "⏸ Pause" : "▶ Resume"}
          </button>
        </div>
      </Section>

      <Section label="Next / Last run">
        <div className={sectionMetaClass}>
          <div>
            next:{" "}
            <span className="font-mono">
              {service.next_run ? `${formatFuture(service.next_run)} (${formatTime(service.next_run)})` : "—"}
            </span>
          </div>
          <div>
            last status:{" "}
            <span className="font-mono">{service.last_run_status ?? "—"}</span>
          </div>
        </div>
      </Section>

      {runs.length > 0 && (
        <Section label="Recent runs">
          <div className="flex flex-col gap-0.5 max-h-28 overflow-y-auto">
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
        </Section>
      )}

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
    </>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className={UI_PANEL.section}>
      <div className={UI_PANEL.sectionLabel}>{label}</div>
      {children}
    </div>
  );
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
