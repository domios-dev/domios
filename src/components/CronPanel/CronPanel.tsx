import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useCrons } from "../../store/crons";
import type { CronRunInfo } from "../../lib/types";
import { ArrowLeftIcon, CloseIcon16 } from "../Icons";
import { describeSchedule, formatTime, SCHEDULE_PRESETS } from "../../lib/formatters";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer } from "../../store/multiplexer";

const HISTORY_VISIBLE = 15;
const LOG_SUMMARY_MARKER = "--- SUMMARY ---";
const LOG_RAW_MARKER = "--- RAW OUTPUT ---";


// ── Types ────────────────────────────────────────────────────────

type CronView = { type: "dashboard" } | { type: "detail"; cronId: string };

interface CronPanelProps {
  initialView: CronView;
  onClose: () => void;
}

export function CronPanel({ initialView, onClose }: CronPanelProps) {
  const [view, setView] = useState<CronView>(initialView);

  // Keep internal view in sync when the caller navigates to a different
  // cron from outside (e.g. sidebar click on another entry while the
  // panel is already open).
  useEffect(() => {
    setView(initialView);
  }, [initialView]);

  return (
    <div className="absolute inset-0 flex flex-col bg-[var(--ct-sidebar-bg)] overflow-hidden">
      {/* Subtle dot-grid background, matches Settings/Memory panels */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          backgroundImage: "radial-gradient(var(--ct-border) 1px, transparent 1px)",
          backgroundSize: "20px 20px",
          opacity: 0.3,
        }}
      />
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--ct-border)] relative z-10">
        <div className="flex items-center gap-2">
          {view.type === "detail" && (
            <button
              onClick={() => setView({ type: "dashboard" })}
              className="flex items-center justify-center w-7 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
              title="Back to all tasks"
            >
              <ArrowLeftIcon />
            </button>
          )}
          <h2 className="text-sm font-medium text-[var(--ct-fg)]">
            {view.type === "dashboard" ? "Periodic Tasks" : "Task Settings"}
          </h2>
        </div>
        <button
          onClick={onClose}
          className="flex items-center justify-center w-7 h-7 rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
        >
          <CloseIcon16 />
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto relative z-10">
        {view.type === "dashboard" ? (
          <CronDashboard onSelectCron={(id) => setView({ type: "detail", cronId: id })} />
        ) : (
          <CronDetail cronId={view.cronId} onBack={() => setView({ type: "dashboard" })} />
        )}
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
  const { providers } = useAgentProviders();
  const { state: muxState } = useMultiplexer();
  const job = crons.find((c) => c.id === cronId);
  const workspace = job
    ? muxState.workspaces.find((w) => w.id === job.workspace_id)
    : undefined;
  // Resolved working directory shown in the placeholder when the user
  // hasn't set an explicit cwd on the job — falls back to the workspace's
  // own cwd (that's what spawn_pane inherits).
  const resolvedDefaultCwd = workspace?.cwd ?? "";

  const [name, setName] = useState(job?.name ?? "");
  const [description, setDescription] = useState(job?.description ?? "");
  const [schedule, setSchedule] = useState(job?.schedule ?? "");
  const [task, setTask] = useState(job?.task ?? "");
  const [cwd, setCwd] = useState(job?.cwd ?? "");
  const [agent, setAgent] = useState(job?.agent ?? "claude");
  const [model, setModel] = useState(job?.model ?? "");
  const [runs, setRuns] = useState<CronRunInfo[]>([]);
  const [showAllRuns, setShowAllRuns] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [logView, setLogView] = useState<{
    runId: string;
    status: CronRunInfo["status"];
  } | null>(null);

  const selectedProvider = useMemo(
    () => providers.find((p) => p.id === agent),
    [providers, agent],
  );
  const availableModels = selectedProvider?.available_models ?? [];

  // Stream the run history: initial load + 2s polling so new runs and
  // status transitions (Running → Success/Failed) show up without the
  // user having to reopen the panel.
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const r = await history(cronId);
        if (!cancelled) setRuns(Array.isArray(r) ? r : []);
      } catch { /* ignore */ }
    };
    load();
    const interval = setInterval(load, 2000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [cronId, history]);

  const handleFolderPick = useCallback(async () => {
    const folder = await open({ directory: true, title: "Select working directory" });
    if (folder) { setCwd(folder as string); setDirty(true); }
  }, []);

  const handleAgentChange = useCallback((value: string) => {
    setAgent(value);
    setModel("");
    setDirty(true);
  }, []);

  const handleSave = useCallback(async () => {
    await update(cronId, {
      name: name.trim() || undefined,
      description: description.trim() || undefined,
      schedule: schedule.trim() || undefined,
      task: task.trim() || undefined,
      cwd: cwd.trim() || undefined,
      agent,
      model: model || null,
    });
    setDirty(false);
  }, [cronId, name, description, schedule, task, cwd, agent, model, update]);

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
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-[11px] text-[var(--ct-muted)]">Schedule</label>
            {/* Compact inline presets — click to fill the input. */}
            <div className="flex gap-1">
              {SCHEDULE_PRESETS.map((p) => (
                <button
                  key={p.value}
                  onClick={() => { setSchedule(p.value); setDirty(true); }}
                  className={`px-1.5 py-0.5 text-[9px] rounded border transition-colors ${
                    schedule === p.value
                      ? "border-[var(--ct-accent)] bg-[var(--ct-accent)]/10 text-[var(--ct-accent)]"
                      : "border-[var(--ct-border)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]"
                  }`}
                  title={p.label}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>
          <input value={schedule} onChange={(e) => { setSchedule(e.target.value); setDirty(true); }} className={`${inputClass} font-mono`} placeholder="*/30 * * * *" />
          <div className="text-[10px] text-[var(--ct-muted)] mt-1">{describeSchedule(schedule)}</div>
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
        <div className="flex items-center justify-between mb-1.5">
          <label className="text-[11px] text-[var(--ct-muted)]">Working directory</label>
          {resolvedDefaultCwd && !cwd && (
            <span className="text-[10px] text-[var(--ct-muted)]">
              inherits from workspace
            </span>
          )}
        </div>
        <div className="flex gap-2">
          <input
            value={cwd}
            onChange={(e) => { setCwd(e.target.value); setDirty(true); }}
            className={`${inputClass} flex-1 font-mono text-[11px]`}
            placeholder={resolvedDefaultCwd || "e.g. C:/Developer/codeterm"}
          />
          <button
            onClick={handleFolderPick}
            className="px-3 py-2 text-[11px] border border-[var(--ct-border)] rounded-md hover:bg-[var(--ct-hover)] text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors shrink-0"
          >
            Browse
          </button>
        </div>
      </div>

      {/* Agent provider + model */}
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Agent</label>
          <select
            value={agent}
            onChange={(e) => handleAgentChange(e.target.value)}
            className={inputClass}
          >
            {providers.length === 0 ? (
              <option value={agent}>{agent}</option>
            ) : (
              providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.display_name}
                </option>
              ))
            )}
          </select>
        </div>
        <div>
          <label className="text-[11px] text-[var(--ct-muted)] block mb-1.5">Model</label>
          <select
            value={model}
            onChange={(e) => { setModel(e.target.value); setDirty(true); }}
            disabled={availableModels.length === 0}
            className={`${inputClass} disabled:opacity-50`}
          >
            <option value="">
              {selectedProvider?.default_model
                ? `Default (${selectedProvider.default_model})`
                : "Default"}
            </option>
            {availableModels.map((m) => (
              <option key={m.id} value={m.id}>
                {m.display_name}
              </option>
            ))}
          </select>
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

      {/* Run history — click any row to open the log popup (live while Running) */}
      <div>
        <label className="text-[11px] text-[var(--ct-muted)] block mb-2">
          Recent runs {runs.length > 0 && `(${runs.length})`}
        </label>
        {runs.length === 0 ? (
          <div className="text-[11px] text-[var(--ct-muted)] px-3 py-2">No runs yet.</div>
        ) : (
          <div className="flex flex-col gap-1">
            {(() => {
              const reversed = [...runs].reverse();
              const visible = showAllRuns ? reversed : reversed.slice(0, HISTORY_VISIBLE);
              const hidden = reversed.length - visible.length;
              return (
                <>
                  {visible.map((run) => {
                    const statusColor =
                      run.status === "Running"
                        ? "text-[var(--ct-accent)] animate-pulse"
                        : run.status === "Success"
                          ? "text-[var(--ct-green)]"
                          : run.status === "Cancelled"
                            ? "text-[var(--ct-muted-fg)]"
                            : "text-[var(--ct-red)]";
                    const icon =
                      run.status === "Running"
                        ? "\u25CF"
                        : run.status === "Success"
                          ? "\u2713"
                          : run.status === "Cancelled"
                            ? "\u25A0"
                            : "\u2717";
                    return (
                      <div
                        key={run.run_id}
                        onClick={() => setLogView({ runId: run.run_id, status: run.status })}
                        className="flex items-center gap-3 px-3 py-1.5 rounded-md bg-[var(--ct-surface)] hover:bg-[var(--ct-hover)] text-[11px] cursor-pointer transition-colors"
                      >
                        <span className={statusColor}>{icon}</span>
                        <span className="text-[var(--ct-fg)] w-16">{run.status}</span>
                        <span className="text-[var(--ct-muted)] flex-1">{formatTime(run.started_at)}</span>
                        <span className="text-[var(--ct-muted-fg)] text-[10px]">view log →</span>
                      </div>
                    );
                  })}
                  {hidden > 0 && (
                    <button
                      onClick={() => setShowAllRuns(true)}
                      className="text-[10px] text-[var(--ct-muted)] hover:text-[var(--ct-accent)] px-3 py-1 text-left transition-colors"
                    >
                      Show {hidden} more…
                    </button>
                  )}
                  {showAllRuns && reversed.length > HISTORY_VISIBLE && (
                    <button
                      onClick={() => setShowAllRuns(false)}
                      className="text-[10px] text-[var(--ct-muted)] hover:text-[var(--ct-accent)] px-3 py-1 text-left transition-colors"
                    >
                      Show less
                    </button>
                  )}
                </>
              );
            })()}
          </div>
        )}
      </div>

      {logView && (
        <CronLogModal
          name={job.name}
          jobId={cronId}
          runId={logView.runId}
          initialStatus={logView.status}
          onClose={() => setLogView(null)}
        />
      )}
    </div>
  );
}

// ── Log popup (live polling while the run is active) ─────────────

function CronLogModal({
  name,
  jobId,
  runId,
  initialStatus,
  onClose,
}: {
  name: string;
  jobId: string;
  runId: string;
  initialStatus: CronRunInfo["status"];
  onClose: () => void;
}) {
  const [showRaw, setShowRaw] = useState(false);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState(initialStatus);
  const scrollRef = useRef<HTMLDivElement>(null);

  const isRunning = status === "Running";

  // Fetch log: saved file for finished runs, falls through to live pane
  // output via cron_read_log's backend fallback for Running runs.
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const t = await invoke<string>("cron_read_log", { jobId, runId });
        if (!cancelled) {
          setText(t);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    };
    load();
    if (!isRunning) return () => { cancelled = true; };
    const interval = setInterval(load, 2000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [jobId, runId, isRunning]);

  // Watch for the run's status flipping away from Running so we stop
  // polling the log and flip the badge.
  useEffect(() => {
    if (!isRunning) return;
    const interval = setInterval(async () => {
      try {
        const h = await invoke<CronRunInfo[]>("cron_history", { id: jobId });
        const run = Array.isArray(h) ? h.find((r) => r.run_id === runId) : undefined;
        if (run && run.status !== "Running") setStatus(run.status);
      } catch { /* ignore */ }
    }, 2000);
    return () => clearInterval(interval);
  }, [jobId, runId, isRunning]);

  // Auto-scroll to bottom as new content arrives.
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [text]);

  // Esc closes the modal.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Finished runs have --- SUMMARY --- / --- RAW OUTPUT --- markers;
  // running runs don't, so show the raw live output as-is.
  let summary = text ?? "";
  let raw = "";
  if (text) {
    const rawIdx = text.indexOf(LOG_RAW_MARKER);
    if (rawIdx !== -1) {
      const sumIdx = text.indexOf(LOG_SUMMARY_MARKER);
      const sumStart = sumIdx !== -1 ? sumIdx + LOG_SUMMARY_MARKER.length : 0;
      summary = text.slice(sumStart, rawIdx).trim();
      raw = text.slice(rawIdx + LOG_RAW_MARKER.length).trim();
    }
  }

  const statusColor =
    status === "Running"
      ? "var(--ct-accent)"
      : status === "Success"
        ? "var(--ct-green)"
        : status === "Cancelled"
          ? "var(--ct-muted-fg)"
          : "var(--ct-red)";

  return (
    <>
      <div className="fixed inset-0 z-[60] bg-black/50" onClick={onClose} />
      <div className="fixed z-[60] top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[720px] max-w-[90vw] max-h-[75vh] flex flex-col overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--ct-border)]">
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-[12px] font-medium text-[var(--ct-fg)] truncate">{name}</span>
            <span
              className="text-[9px] px-1.5 py-[1px] rounded-full shrink-0 uppercase"
              style={{ color: statusColor, border: `1px solid ${statusColor}` }}
            >
              {isRunning ? `${status}…` : status}
            </span>
          </div>
          <button onClick={onClose} className="text-[var(--ct-muted)] hover:text-[var(--ct-fg)]">
            {"\u00D7"}
          </button>
        </div>
        <div ref={scrollRef} className="flex-1 overflow-auto p-4">
          {text === null && !error && (
            <span className="text-[11px] text-[var(--ct-muted)]">Loading…</span>
          )}
          {error && (
            <span className="text-[11px] text-[var(--ct-red)]">Error: {error}</span>
          )}
          {text !== null && (
            <pre className="text-[11px] font-mono text-[var(--ct-fg)] whitespace-pre-wrap leading-relaxed m-0">
              {summary || text}
            </pre>
          )}
          {raw && (
            <div className="mt-4 border-t border-[var(--ct-border)] pt-3">
              <button
                onClick={() => setShowRaw(!showRaw)}
                className="text-[10px] text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] transition-colors flex items-center gap-1"
              >
                <span>{showRaw ? "\u25BC" : "\u25B6"}</span>
                Raw terminal output
              </button>
              {showRaw && (
                <pre className="mt-2 text-[10px] font-mono text-[var(--ct-muted-fg)] whitespace-pre-wrap leading-relaxed m-0">
                  {raw}
                </pre>
              )}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
