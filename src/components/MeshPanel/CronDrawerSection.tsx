import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { CronRunInfo } from "../../lib/types";
import { describeSchedule, formatTime } from "../../lib/formatters";
import type { Service } from "./types";

interface Props {
  service: Service;
  onUpdate: () => void;
}

export function CronDrawerSection({ service, onUpdate }: Props) {
  const [editingSchedule, setEditingSchedule] = useState(false);
  const [scheduleInput, setScheduleInput] = useState(service.schedule ?? "");
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<CronRunInfo[]>("cron_history", { id: service.id })
      .then((r) => { if (!cancelled) setRuns(Array.isArray(r) ? r.slice(-5).reverse() : []); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [service.id]);

  const handleTrigger = async () => {
    setBusy(true);
    try {
      await invoke("cron_trigger", { id: service.id });
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
      await invoke("cron_toggle", { id: service.id, enabled: !service.enabled });
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
      await invoke("cron_update", { id: service.id, schedule: scheduleInput.trim() });
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
              className="flex-1 px-2 py-1 text-[10px] font-mono bg-[var(--ct-bg)] border border-[var(--ct-accent)] rounded text-[var(--ct-fg)] outline-none"
              placeholder="*/30 * * * *"
            />
            <button
              onClick={handleSaveSchedule}
              disabled={busy}
              className="px-2 py-1 text-[10px] bg-[var(--ct-accent)] text-[var(--ct-bg)] rounded disabled:opacity-50 cursor-pointer"
            >
              Save
            </button>
            <button
              onClick={() => { setEditingSchedule(false); setScheduleInput(service.schedule ?? ""); }}
              className="px-2 py-1 text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[var(--ct-muted)] rounded cursor-pointer"
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
              className="text-[9px] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] bg-transparent border-none cursor-pointer"
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
            className="flex-1 py-1 text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[var(--ct-fg)] rounded cursor-pointer disabled:opacity-50 hover:bg-[var(--ct-hover)]"
          >
            ▶ Trigger now
          </button>
          <button
            onClick={handleToggle}
            disabled={busy}
            className="flex-1 py-1 text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] text-[var(--ct-fg)] rounded cursor-pointer disabled:opacity-50 hover:bg-[var(--ct-hover)]"
          >
            {service.enabled ? "⏸ Pause" : "▶ Resume"}
          </button>
        </div>
      </Section>

      <Section label="Next / Last run">
        <div className="text-[10px] text-[var(--ct-fg)] flex flex-col gap-0.5">
          <div>
            next:{" "}
            <span className="font-mono">
              {service.next_run ? formatTime(service.next_run * 1000) : "—"}
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
            {runs.map((run) => (
              <div key={run.run_id} className="flex items-center gap-2 text-[10px]">
                <span className={runColor(run.status)}>{runIcon(run.status)}</span>
                <span className="text-[var(--ct-muted)] flex-1">
                  {formatTime(run.started_at)}
                </span>
                {run.finished_at && (
                  <span className="text-[var(--ct-muted-fg)] font-mono">
                    {Math.round((run.finished_at - run.started_at) / 1000)}s
                  </span>
                )}
              </div>
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="text-[10px] flex flex-col gap-1">
      <div className="text-[var(--ct-muted)] uppercase tracking-wide text-[9px]">{label}</div>
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
