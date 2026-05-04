import { memo, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig, CronJob, CronRunInfo, GraphWorkerInfo } from "../../lib/types";
import { baseUrl, authHeaders, isTauriEnv } from "./memoryApi";
import { WorkerLogPopup } from "./WorkerLogPopup";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";

interface Props {
  api: ApiConfig;
}

interface DreamDiaryEntry {
  id: string;
  title: string;
  body: string;
  created_at: string;
  area: string | null;
  tags?: string[];
}

interface SearchHit {
  item: DreamDiaryEntry;
  score: number;
}

function relativeTime(target: number): string {
  const diff = target - Date.now();
  const abs = Math.abs(diff);
  const past = diff < 0;
  const mins = Math.floor(abs / 60000);
  if (mins < 1) return past ? "just now" : "in <1m";
  if (mins < 60) return past ? `${mins}m ago` : `in ${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return past ? `${hrs}h ago` : `in ${hrs}h`;
  const days = Math.floor(hrs / 24);
  return past ? `${days}d ago` : `in ${days}d`;
}

function formatDuration(startMs: number, endMs: number): string {
  const diff = endMs - startMs;
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return `${mins}m ${rem}s`;
}

function formatDateTime(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

interface MemoryConfig {
  dreaming_enabled: boolean;
  dreaming_agent: string;
  dreaming_model: string;
}

interface ProviderInfo {
  id: string;
  display_name: string;
  available_models?: Array<{ id: string; display_name: string }>;
}

type RunHistoryItem =
  | { type: "dream"; worker: GraphWorkerInfo }
  | { type: "librarian"; run: CronRunInfo };

function statusColor(status: string): string {
  switch (status) {
    case "done":
    case "Success":
      return "#10b981";
    case "error":
    case "Failed":
      return "#ef4444";
    case "Cancelled":
    case "orphaned":
      return "#9ca3af";
    default:
      return "#eab308";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "Success":
      return "done";
    case "Failed":
      return "error";
    case "Running":
      return "running";
    case "Cancelled":
      return "cancelled";
    default:
      return status;
  }
}

function scheduleLabel(schedule: string): string {
  if (schedule.includes("0 3 * * *") || schedule.includes("0 2 * * *") || schedule.includes("0 4 * * *")) {
    return "Every night ~3 AM";
  }
  if (schedule.includes("0 * * * *")) {
    return "Every hour";
  }
  return schedule;
}

function MemoryScheduledTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [librarianCron, setLibrarianCron] = useState<CronJob | null>(null);
  const [cronLoading, setCronLoading] = useState(true);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [librarianHistory, setLibrarianHistory] = useState<CronRunInfo[]>([]);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [logCronRun, setLogCronRun] = useState<CronRunInfo | null>(null);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const mountedRef = useRef(true);

  const [memCfg, setMemCfg] = useState<MemoryConfig | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [defaultProvider, setDefaultProvider] = useState<string>("claude");

  useEffect(() => {
    fetch(`${baseUrl(api)}/api/config?key=memory`, { headers: authHeaders(api) })
      .then((r) => r.ok ? r.json() : null)
      .then((d) => { if (d?.value && mountedRef.current) setMemCfg(d.value); })
      .catch(() => {});
    fetch(`${baseUrl(api)}/api/config?key=general_agent`, { headers: authHeaders(api) })
      .then((r) => r.ok ? r.json() : null)
      .then((d) => {
        const p = d?.value?.agent_provider;
        if (p && mountedRef.current) setDefaultProvider(p);
      })
      .catch(() => {});
    fetch(`${baseUrl(api)}/api/agents/providers`, { headers: authHeaders(api) })
      .then((r) => r.ok ? r.json() : [])
      .then((p) => { if (mountedRef.current) setProviders(p); })
      .catch(() => {});
  }, [api]);

  const patchMemConfig = useCallback((patch: Partial<MemoryConfig>) => {
    if (!memCfg) return;
    const next = { ...memCfg, ...patch };
    setMemCfg(next);
    fetch(`${baseUrl(api)}/api/config`, {
      method: "PATCH",
      headers: { ...authHeaders(api), "Content-Type": "application/json" },
      body: JSON.stringify({ key: "memory", value: next }),
    }).catch(() => {});
  }, [api, memCfg]);

  const toggleCronEnabled = useCallback(async (cronId: string, enable: boolean) => {
    const endpoint = enable ? "enable" : "disable";
    try {
      const res = await fetch(`${baseUrl(api)}/api/crons/${encodeURIComponent(cronId)}/${endpoint}`, {
        method: "POST",
        headers: authHeaders(api),
      });
      if (res.ok && mountedRef.current) {
        const updated = (await res.json()) as CronJob;
        if (updated.name === "mem-dreaming-auto") setDreamCron(updated);
        else if (updated.name === "mem-librarian-auto") setLibrarianCron(updated);
      }
    } catch { /* network error */ }
  }, [api]);

  useEffect(() => {
    return () => { mountedRef.current = false; };
  }, []);

  const fetchCronData = useCallback(async (showLoading = false) => {
    if (showLoading) setCronLoading(true);
    try {
      const res = await fetch(`${baseUrl(api)}/api/crons`, { headers: authHeaders(api) });
      if (res.ok && mountedRef.current) {
        const jobs = (await res.json()) as CronJob[];
        const dream = jobs.find(j => j.name === "mem-dreaming-auto") ?? null;
        const lib = jobs.find(j => j.name === "mem-librarian-auto") ?? null;
        setDreamCron(dream);
        setLibrarianCron(lib);

        if (lib) {
          const hRes = await fetch(`${baseUrl(api)}/api/crons/${encodeURIComponent(lib.id)}/history`, {
            headers: authHeaders(api),
          });
          if (hRes.ok && mountedRef.current) {
            const runs = (await hRes.json()) as CronRunInfo[];
            setLibrarianHistory(runs.sort((a, b) => b.started_at - a.started_at));
          }
        }
      }
      const wRes = await fetch(`${baseUrl(api)}/api/mem/workers`, { headers: authHeaders(api) });
      if (wRes.ok && mountedRef.current) {
        const workers = (await wRes.json()) as GraphWorkerInfo[];
        const dreamRuns = workers
          .filter(w => w.intent.startsWith("dreaming:"))
          .sort((a, b) => b.spawned_at - a.spawned_at);
        setDreamWorkers(dreamRuns);
      }
    } catch {
      // network error
    } finally {
      if (mountedRef.current && showLoading) setCronLoading(false);
    }
  }, [api]);

  const fetchDiary = useCallback(async (showLoading = false) => {
    if (showLoading) setDiaryLoading(true);
    try {
      const params = new URLSearchParams({
        q: "dream",
        kind: "daily",
        limit: "20",
      });
      const res = await fetch(`${baseUrl(api)}/api/mem/search?${params}`, {
        headers: authHeaders(api),
      });
      if (!res.ok || !mountedRef.current) return;
      const data = (await res.json()) as { hits: SearchHit[] };
      const entries = (data.hits || []).map(h => h.item);
      entries.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
      setDiaryEntries(entries);
    } catch {
      // network error
    } finally {
      if (mountedRef.current && showLoading) setDiaryLoading(false);
    }
  }, [api]);

  useEffect(() => { fetchCronData(true); }, [fetchCronData]);
  useEffect(() => { fetchDiary(true); }, [fetchDiary]);

  useEffect(() => {
    if (!isTauriEnv()) return;
    let active = true;
    const unlisteners: Array<() => void> = [];
    let workerTimer: ReturnType<typeof setTimeout> | null = null;
    let diaryTimer: ReturnType<typeof setTimeout> | null = null;
    const refreshWorkers = () => {
      if (!active) return;
      if (workerTimer) clearTimeout(workerTimer);
      workerTimer = setTimeout(() => { if (active) fetchCronData(false); }, 500);
    };
    const refreshDiary = () => {
      if (!active) return;
      if (diaryTimer) clearTimeout(diaryTimer);
      diaryTimer = setTimeout(() => { if (active) fetchDiary(false); }, 500);
    };
    (async () => {
      try {
        for (const name of ["graph-worker-spawned", "graph-worker-progress", "graph-worker-reported", "graph-worker-done"]) {
          const fn = await listen(name, refreshWorkers);
          if (!active) { fn(); return; }
          unlisteners.push(fn);
        }
        const memFn = await listen<{ op: string }>("mem-activity", (ev) => {
          if (ev.payload?.op === "read") return;
          refreshDiary();
        });
        if (!active) { memFn(); return; }
        unlisteners.push(memFn);
        // Also listen for cron events to refresh librarian runs
        const cronFn = await listen("cron-run-finished", refreshWorkers);
        if (!active) { cronFn(); return; }
        unlisteners.push(cronFn);
      } catch { /* not in tauri */ }
    })();
    return () => {
      active = false;
      if (workerTimer) clearTimeout(workerTimer);
      if (diaryTimer) clearTimeout(diaryTimer);
      unlisteners.forEach(fn => fn());
    };
  }, [fetchCronData, fetchDiary]);

  const mergedHistory: RunHistoryItem[] = [
    ...dreamWorkers.map((w): RunHistoryItem => ({ type: "dream", worker: w })),
    ...librarianHistory.map((r): RunHistoryItem => ({ type: "librarian", run: r })),
  ].sort((a, b) => {
    const timeA = a.type === "dream" ? a.worker.spawned_at : a.run.started_at;
    const timeB = b.type === "dream" ? b.worker.spawned_at : b.run.started_at;
    return timeB - timeA;
  });

  const lastDreamRun = dreamWorkers[0] ?? null;
  const lastLibrarianRun = librarianHistory[0] ?? null;

  return (
    <div className="absolute inset-0 flex flex-col overflow-y-auto">
      <div className="p-3 space-y-4">
        {/* Operation cards */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {/* Dreaming (REM) card */}
          <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 overflow-hidden">
            <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--ct-border)]">
              <span className="text-xs font-semibold text-[var(--ct-fg)]">Dreaming (REM)</span>
              {dreamCron && (
                <label className="flex items-center gap-1.5 cursor-pointer">
                  <span className="text-[10px] text-[var(--ct-muted-fg)]">
                    {dreamCron.enabled ? "On" : "Off"}
                  </span>
                  <input
                    type="checkbox"
                    checked={dreamCron.enabled}
                    onChange={(e) => toggleCronEnabled(dreamCron.id, e.target.checked)}
                    className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                  />
                </label>
              )}
            </div>
            <div className="px-3 py-2 space-y-2">
              {/* Provider / model row */}
              {memCfg && (
                <div className="flex flex-wrap items-center gap-2">
                  <select
                    value={memCfg.dreaming_agent || ""}
                    onChange={(e) => patchMemConfig({ dreaming_agent: e.target.value })}
                    className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[10px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                  >
                    <option value="">Default ({providers.find((p) => p.id === defaultProvider)?.display_name ?? defaultProvider})</option>
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>{p.display_name}</option>
                    ))}
                  </select>
                  {(() => {
                    // Filter models to the resolved provider — when "Default" is
                    // selected, fall through to the configured general_agent
                    // provider, NOT providers[0] (which silently picked the
                    // first registered plugin alphabetically).
                    const resolvedProviderId = memCfg.dreaming_agent || defaultProvider;
                    const prov = providers.find((p) => p.id === resolvedProviderId);
                    const models = prov?.available_models ?? [];
                    return models.length > 0 ? (
                      <select
                        value={memCfg.dreaming_model || ""}
                        onChange={(e) => patchMemConfig({ dreaming_model: e.target.value })}
                        className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[10px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                      >
                        <option value="">Default model</option>
                        {models.map((m) => (
                          <option key={m.id} value={m.id}>{m.display_name}</option>
                        ))}
                      </select>
                    ) : (
                      <input
                        value={memCfg.dreaming_model || ""}
                        onChange={(e) => patchMemConfig({ dreaming_model: e.target.value })}
                        placeholder="Model"
                        className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[10px] text-[var(--ct-fg)] w-28 focus:border-[var(--ct-accent)] focus:outline-none"
                      />
                    );
                  })()}
                </div>
              )}
              {/* Schedule + next run + last run */}
              <div className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)]">
                <span>{dreamCron ? scheduleLabel(dreamCron.schedule) : "Not configured"}</span>
                <span className="opacity-40">|</span>
                <span className="font-mono tabular-nums">
                  {dreamCron?.next_run ? relativeTime(dreamCron.next_run) : "---"}
                </span>
              </div>
              {/* Last run status */}
              {lastDreamRun && (
                <div className="flex items-center gap-2">
                  <span
                    className="px-1.5 py-px rounded-[3px] text-[9px] font-semibold uppercase"
                    style={{
                      background: `${statusColor(lastDreamRun.status)}22`,
                      color: statusColor(lastDreamRun.status),
                    }}
                  >
                    {lastDreamRun.status}
                  </span>
                  <span className="text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
                    {formatDateTime(lastDreamRun.spawned_at)}
                  </span>
                  {lastDreamRun.done_at && (
                    <span className="text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
                      {formatDuration(lastDreamRun.spawned_at, lastDreamRun.done_at)}
                    </span>
                  )}
                </div>
              )}
              {!lastDreamRun && !cronLoading && (
                <div className="text-[10px] text-[var(--ct-muted-fg)]">No runs yet</div>
              )}
            </div>
          </div>

          {/* Librarian (File Sync) card */}
          <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/40 overflow-hidden">
            <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--ct-border)]">
              <span className="text-xs font-semibold text-[var(--ct-fg)]">Librarian (File Sync)</span>
              {librarianCron && (
                <label className="flex items-center gap-1.5 cursor-pointer">
                  <span className="text-[10px] text-[var(--ct-muted-fg)]">
                    {librarianCron.enabled ? "On" : "Off"}
                  </span>
                  <input
                    type="checkbox"
                    checked={librarianCron.enabled}
                    onChange={(e) => toggleCronEnabled(librarianCron.id, e.target.checked)}
                    className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                  />
                </label>
              )}
            </div>
            <div className="px-3 py-2 space-y-2">
              {/* No model selector for librarian — just provider info */}
              <div className="text-[10px] text-[var(--ct-muted-fg)]">
                Watches source-tracked memory items and re-indexes on change
              </div>
              {/* Schedule + next run */}
              <div className="flex items-center gap-2 text-[10px] text-[var(--ct-muted-fg)]">
                <span>{librarianCron ? scheduleLabel(librarianCron.schedule) : "Not configured"}</span>
                <span className="opacity-40">|</span>
                <span className="font-mono tabular-nums">
                  {librarianCron?.next_run ? relativeTime(librarianCron.next_run) : "---"}
                </span>
              </div>
              {/* Last run status */}
              {lastLibrarianRun && (
                <div className="flex items-center gap-2">
                  <span
                    className="px-1.5 py-px rounded-[3px] text-[9px] font-semibold uppercase"
                    style={{
                      background: `${statusColor(lastLibrarianRun.status)}22`,
                      color: statusColor(lastLibrarianRun.status),
                    }}
                  >
                    {statusLabel(lastLibrarianRun.status)}
                  </span>
                  <span className="text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
                    {formatDateTime(lastLibrarianRun.started_at)}
                  </span>
                  {lastLibrarianRun.finished_at && (
                    <span className="text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
                      {formatDuration(lastLibrarianRun.started_at, lastLibrarianRun.finished_at)}
                    </span>
                  )}
                </div>
              )}
              {!lastLibrarianRun && !cronLoading && (
                <div className="text-[10px] text-[var(--ct-muted-fg)]">No runs yet</div>
              )}
              {!librarianCron && !cronLoading && (
                <div className="text-[10px] text-[var(--ct-muted-fg)] italic">
                  No librarian cron found
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Unified run history */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Run History
          </h3>
          {mergedHistory.length === 0 && !cronLoading && (
            <div className="text-xs text-[var(--ct-muted-fg)]">No runs yet</div>
          )}
          <div className="space-y-1">
            {mergedHistory.map(item => {
              if (item.type === "dream") {
                const w = item.worker;
                const isDone = w.status === "done" || w.status === "error" || w.status === "orphaned";
                const color = statusColor(w.status);
                const dur = isDone && w.done_at ? formatDuration(w.spawned_at, w.done_at) : "";
                return (
                  <div
                    key={`dream-${w.worker_id}`}
                    className="flex items-center gap-2.5 px-3 py-1.5 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] cursor-pointer hover:border-[var(--ct-accent)]"
                    onClick={() => setLogWorker(w)}
                  >
                    <span
                      className="px-1.5 py-px rounded-[3px] text-[9px] font-semibold uppercase shrink-0"
                      style={{ background: "#8b5cf622", color: "#8b5cf6" }}
                    >
                      REM
                    </span>
                    <span
                      className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase w-[52px] text-center shrink-0"
                      style={{ background: `${color}22`, color }}
                    >
                      {w.status}
                    </span>
                    <span className="text-[11px] text-[var(--ct-fg)] font-mono tabular-nums">
                      {formatDateTime(w.spawned_at)}
                    </span>
                    {w.model && (
                      <span className="text-[9px] text-[var(--ct-muted-fg)]">{w.model}</span>
                    )}
                    {dur && (
                      <span className="text-[10px] text-[var(--ct-muted-fg)] ml-auto tabular-nums">{dur}</span>
                    )}
                    {!isDone && (
                      <span className="text-[10px] ml-auto flex items-center gap-1" style={{ color }}>
                        <span className="inline-block w-1.5 h-1.5 rounded-full animate-pulse" style={{ background: color }} />
                        running
                      </span>
                    )}
                  </div>
                );
              }
              const r = item.run;
              const color = statusColor(r.status);
              const isTerminal = r.status !== "Running";
              const dur = isTerminal && r.finished_at ? formatDuration(r.started_at, r.finished_at) : "";
              return (
                <div
                  key={`lib-${r.run_id}`}
                  className="flex items-center gap-2.5 px-3 py-1.5 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] cursor-pointer hover:border-[var(--ct-accent)]"
                  onClick={() => r.pane_id ? setLogCronRun(r) : undefined}
                >
                  <span
                    className="px-1.5 py-px rounded-[3px] text-[9px] font-semibold uppercase shrink-0"
                    style={{ background: "#06b6d422", color: "#06b6d4" }}
                  >
                    Sync
                  </span>
                  <span
                    className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase w-[52px] text-center shrink-0"
                    style={{ background: `${color}22`, color }}
                  >
                    {statusLabel(r.status)}
                  </span>
                  <span className="text-[11px] text-[var(--ct-fg)] font-mono tabular-nums">
                    {formatDateTime(r.started_at)}
                  </span>
                  {dur && (
                    <span className="text-[10px] text-[var(--ct-muted-fg)] ml-auto tabular-nums">{dur}</span>
                  )}
                  {!isTerminal && (
                    <span className="text-[10px] ml-auto flex items-center gap-1" style={{ color }}>
                      <span className="inline-block w-1.5 h-1.5 rounded-full animate-pulse" style={{ background: color }} />
                      running
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </section>

        {/* Dream diary */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Dream Diary
          </h3>
          {diaryLoading ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">Loading diary...</div>
          ) : diaryEntries.length === 0 ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">
              No dream diary entries. Dreams will appear here after the first dreaming run.
            </div>
          ) : (
            <div className="space-y-2">
              {diaryEntries.map(entry => (
                <div
                  key={entry.id}
                  className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]"
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-[11px] font-semibold text-[var(--ct-fg)]">
                      {entry.title}
                    </span>
                    <span className="text-[10px] text-[var(--ct-muted-fg)] ml-auto tabular-nums">
                      {new Date(entry.created_at).toLocaleString("en-US", {
                        month: "short",
                        day: "numeric",
                        year: "numeric",
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </span>
                  </div>
                  <pre className="text-[11px] text-[var(--ct-muted-fg)] whitespace-pre-wrap font-sans leading-relaxed m-0">
                    {entry.body}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
      {logWorker && (
        <WorkerLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />
      )}
      {logCronRun && (
        <AgentRunLogPopup
          api={api}
          paneId={logCronRun.pane_id}
          title="Librarian Sync"
          status={logCronRun.status}
          onClose={() => setLogCronRun(null)}
        />
      )}
    </div>
  );
}

export const MemoryScheduledTab = memo(MemoryScheduledTabImpl);
