import { useState, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useCrons } from "../../store/crons";
import type { CronRunInfo } from "../../lib/types";
import { ArrowLeftIcon, CloseIcon16 } from "../Icons";
import { describeSchedule, formatTime, SCHEDULE_PRESETS } from "../../lib/formatters";


// ── Types ────────────────────────────────────────────────────────

type CronView = { type: "dashboard" } | { type: "detail"; cronId: string };

interface CronPanelProps {
  initialView: CronView;
  onClose: () => void;
}

export function CronPanel({ initialView, onClose }: CronPanelProps) {
  const [view, setView] = useState<CronView>(initialView);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-xl shadow-2xl w-[640px] max-h-[80vh] flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2">
            {view.type === "detail" && (
              <button
                onClick={() => setView({ type: "dashboard" })}
                className="text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors mr-1"
                title="Back to all tasks"
              >
                <ArrowLeftIcon />
              </button>
            )}
            <span className="text-[13px] font-medium text-[var(--ct-fg)]">
              {view.type === "dashboard" ? "Periodic Tasks" : "Task Settings"}
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          >
            <CloseIcon16 />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          {view.type === "dashboard" ? (
            <CronDashboard onSelectCron={(id) => setView({ type: "detail", cronId: id })} />
          ) : (
            <CronDetail cronId={view.cronId} onBack={() => setView({ type: "dashboard" })} />
          )}
        </div>
      </div>
    </div>
  );
}

// ── Dashboard (tile grid) ────────────────────────────────────────

function CronDashboard({ onSelectCron }: { onSelectCron: (id: string) => void }) {
  const { crons, toggle, trigger, remove } = useCrons();

  if (crons.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-[var(--ct-muted)]">
        <span className="text-3xl mb-3">{"\u23F1"}</span>
        <span className="text-[13px]">No periodic tasks yet</span>
        <span className="text-[11px] mt-1">Create one via CLI: <code className="text-[var(--ct-accent)]">codeterm cron create</code></span>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-3 p-4">
      {crons.map((job) => (
        <div
          key={job.id}
          onClick={() => onSelectCron(job.id)}
          className={`group relative flex flex-col gap-2 p-4 rounded-lg border cursor-pointer transition-all hover:border-[var(--ct-accent)]/50 hover:bg-[var(--ct-hover)] ${
            job.enabled
              ? "border-[var(--ct-border)] bg-[var(--ct-surface)]"
              : "border-[var(--ct-border)] bg-[var(--ct-surface)] opacity-50"
          }`}
        >
          {/* Top row: icon + name + status */}
          <div className="flex items-center gap-2">
            <span className="text-[16px]">{"\u23F1"}</span>
            <span className="text-[12px] font-medium text-[var(--ct-fg)] truncate flex-1">{job.name}</span>
            <span className={`w-2 h-2 rounded-full shrink-0 ${
              !job.enabled ? "bg-[var(--ct-muted)]" :
              job.last_run?.status === "Running" ? "bg-[var(--ct-accent)] animate-pulse" :
              "bg-[var(--ct-green)]"
            }`} />
          </div>

          {/* Schedule */}
          <div className="text-[11px] text-[var(--ct-muted)] font-mono">{describeSchedule(job.schedule)}</div>

          {/* Task preview */}
          <div className="text-[10px] text-[var(--ct-muted-fg)] line-clamp-2 leading-relaxed">{job.task}</div>

          {/* Footer: next run + actions */}
          <div className="flex items-center justify-between mt-1">
            <span className="text-[10px] text-[var(--ct-muted)]">
              {!job.enabled ? "disabled" : job.next_run ? `next ${formatTime(job.next_run)}` : "scheduled"}
            </span>

            {/* Hover actions */}
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity" onClick={(e) => e.stopPropagation()}>
              <button
                onClick={() => trigger(job.id)}
                className="px-1.5 py-0.5 text-[10px] rounded border border-[var(--ct-border)] hover:bg-[var(--ct-active)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                title="Trigger now"
              >
                {"\u25B6"}
              </button>
              <button
                onClick={() => toggle(job.id, !job.enabled)}
                className="px-1.5 py-0.5 text-[10px] rounded border border-[var(--ct-border)] hover:bg-[var(--ct-active)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                title={job.enabled ? "Disable" : "Enable"}
              >
                {job.enabled ? "\u23F8" : "\u25B6"}
              </button>
              <button
                onClick={() => remove(job.id)}
                className="px-1.5 py-0.5 text-[10px] rounded border border-[var(--ct-border)] hover:bg-[var(--ct-active)] text-[var(--ct-muted)] hover:text-[var(--ct-red)]"
                title="Delete"
              >
                {"\u00D7"}
              </button>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

// ── Detail (cron settings) ───────────────────────────────────────

function CronDetail({ cronId, onBack }: { cronId: string; onBack: () => void }) {
  const { crons, update, toggle, trigger, remove, history } = useCrons();
  const job = crons.find((c) => c.id === cronId);

  const [name, setName] = useState(job?.name ?? "");
  const [description, setDescription] = useState(job?.description ?? "");
  const [schedule, setSchedule] = useState(job?.schedule ?? "");
  const [task, setTask] = useState(job?.task ?? "");
  const [cwd, setCwd] = useState(job?.cwd ?? "");
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [dirty, setDirty] = useState(false);

  // Load history on mount
  useState(() => {
    history(cronId).then((r) => setRuns(Array.isArray(r) ? r : []));
  });

  const handleFolderPick = useCallback(async () => {
    const folder = await open({ directory: true, title: "Select working directory" });
    if (folder) { setCwd(folder as string); setDirty(true); }
  }, []);

  const handleSave = useCallback(async () => {
    await update(cronId, {
      name: name.trim() || undefined,
      description: description.trim() || undefined,
      schedule: schedule.trim() || undefined,
      task: task.trim() || undefined,
      cwd: cwd.trim() || undefined,
    });
    setDirty(false);
  }, [cronId, name, description, schedule, task, cwd, update]);

  const handleDelete = useCallback(async () => {
    await remove(cronId);
    onBack();
  }, [cronId, remove, onBack]);

  if (!job) {
    return (
      <div className="p-8 text-center text-[var(--ct-muted)]">Task not found</div>
    );
  }

  const inputClass = "w-full px-3 py-2 text-[12px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-md text-[var(--ct-fg)] outline-none focus:border-[var(--ct-accent)] transition-colors";

  return (
    <div className="p-5 flex flex-col gap-4">
      {/* Status bar */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-[11px] rounded-full border ${
            job.enabled
              ? "border-[var(--ct-green)]/30 bg-[var(--ct-green)]/10 text-[var(--ct-green)]"
              : "border-[var(--ct-muted)]/30 bg-[var(--ct-muted)]/10 text-[var(--ct-muted)]"
          }`}>
            <span className={`w-1.5 h-1.5 rounded-full ${job.enabled ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
            {job.enabled ? "Enabled" : "Disabled"}
          </span>
          <span className="text-[11px] text-[var(--ct-muted)] font-mono">{describeSchedule(job.schedule)}</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => trigger(cronId)}
            className="px-3 py-1.5 text-[11px] rounded-md border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] text-[var(--ct-fg)] transition-colors"
          >
            {"\u25B6"} Trigger now
          </button>
          <button
            onClick={() => toggle(cronId, !job.enabled)}
            className="px-3 py-1.5 text-[11px] rounded-md border border-[var(--ct-border)] hover:bg-[var(--ct-hover)] text-[var(--ct-fg)] transition-colors"
          >
            {job.enabled ? "\u23F8 Disable" : "\u25B6 Enable"}
          </button>
        </div>
      </div>

      {/* Fields */}
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Name</label>
          <input value={name} onChange={(e) => { setName(e.target.value); setDirty(true); }} className={inputClass} />
        </div>
        <div>
          <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Schedule</label>
          <input value={schedule} onChange={(e) => { setSchedule(e.target.value); setDirty(true); }} className={`${inputClass} font-mono`} placeholder="*/30 * * * *" />
          <div className="text-[10px] text-[var(--ct-muted)] mt-1">{describeSchedule(schedule)}</div>
        </div>
      </div>

      {/* Presets */}
      <div>
        <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Quick presets</label>
        <div className="flex gap-1.5 flex-wrap">
          {SCHEDULE_PRESETS.map((p) => (
            <button
              key={p.value}
              onClick={() => { setSchedule(p.value); setDirty(true); }}
              className={`px-2 py-1 text-[10px] rounded border transition-colors ${
                schedule === p.value
                  ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10 text-[var(--ct-accent)]"
                  : "border-[var(--ct-border)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
              }`}
            >
              {p.label}
            </button>
          ))}
        </div>
      </div>

      <div>
        <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Description</label>
        <input value={description} onChange={(e) => { setDescription(e.target.value); setDirty(true); }} className={inputClass} placeholder="Optional" />
      </div>

      <div>
        <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Task prompt</label>
        <textarea value={task} onChange={(e) => { setTask(e.target.value); setDirty(true); }} rows={3} className={`${inputClass} resize-none`} />
      </div>

      <div>
        <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Working directory</label>
        <div className="flex gap-2">
          <input value={cwd} onChange={(e) => { setCwd(e.target.value); setDirty(true); }} className={`${inputClass} flex-1`} placeholder="Default" />
          <button onClick={handleFolderPick} className="px-3 py-2 text-[11px] border border-[var(--ct-border)] rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors shrink-0">
            Browse
          </button>
        </div>
      </div>

      {/* Save / Delete */}
      <div className="flex items-center justify-between pt-2 border-t border-[var(--ct-border)]">
        <button
          onClick={handleDelete}
          className="px-3 py-1.5 text-[11px] text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 rounded-md transition-colors"
        >
          Delete task
        </button>
        <button
          onClick={handleSave}
          disabled={!dirty}
          className="px-4 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 transition-opacity disabled:opacity-30"
        >
          Save changes
        </button>
      </div>

      {/* Run history */}
      {runs.length > 0 && (
        <div>
          <label className="text-[11px] text-[var(--ct-muted)] block mb-2">Recent runs</label>
          <div className="flex flex-col gap-1">
            {[...runs].reverse().map((run) => (
              <div key={run.run_id} className="flex items-center gap-3 px-3 py-1.5 rounded-md bg-[var(--ct-surface)] text-[11px]">
                <span className={
                  run.status === "Running" ? "text-[var(--ct-accent)] animate-pulse" :
                  run.status === "Success" ? "text-[var(--ct-green)]" : "text-[var(--ct-red)]"
                }>
                  {run.status === "Running" ? "\u25CF" : run.status === "Success" ? "\u2713" : "\u2717"}
                </span>
                <span className="text-[var(--ct-fg)]">{run.status}</span>
                <span className="text-[var(--ct-muted)]">{formatTime(run.started_at)}</span>
                {run.pane_id && <span className="text-[var(--ct-muted)] font-mono text-[10px]">pane:{run.pane_id.slice(0, 8)}</span>}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
