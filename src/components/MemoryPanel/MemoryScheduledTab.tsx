import { memo, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig, CronJob, GraphWorkerInfo } from "../../lib/types";
import { baseUrl, authHeaders, isTauriEnv } from "./memoryApi";
import { WorkerLogPopup } from "./WorkerLogPopup";

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

function MemoryScheduledTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [cronLoading, setCronLoading] = useState(true);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const mountedRef = useRef(true);

  const [memCfg, setMemCfg] = useState<MemoryConfig | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);

  useEffect(() => {
    fetch(`${baseUrl(api)}/api/config?key=memory`, { headers: authHeaders(api) })
      .then((r) => r.ok ? r.json() : null)
      .then((d) => { if (d?.value && mountedRef.current) setMemCfg(d.value); })
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

  useEffect(() => {
    return () => { mountedRef.current = false; };
  }, []);

  const fetchCronData = useCallback(async (showLoading = false) => {
    if (showLoading) setCronLoading(true);
    try {
      const res = await fetch(`${baseUrl(api)}/api/crons`, { headers: authHeaders(api) });
      if (res.ok && mountedRef.current) {
        const jobs = (await res.json()) as CronJob[];
        setDreamCron(jobs.find(j => j.name === "mem-dreaming-auto") ?? null);
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

  // Live updates: debounced re-fetch on worker events and mem activity
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
        // Only refetch on writes, not reads (safety net)
        const memFn = await listen<{ op: string }>("mem-activity", (ev) => {
          if (ev.payload?.op === "read") return;
          refreshDiary();
        });
        if (!active) { memFn(); return; }
        unlisteners.push(memFn);
      } catch { /* not in tauri */ }
    })();
    return () => {
      active = false;
      if (workerTimer) clearTimeout(workerTimer);
      if (diaryTimer) clearTimeout(diaryTimer);
      unlisteners.forEach(fn => fn());
    };
  }, [fetchCronData, fetchDiary]);

  return (
    <div className="absolute inset-0 flex flex-col overflow-y-auto">
      <div className="p-3 space-y-4">
        {/* Dreaming (REM) settings */}
        {memCfg && (
          <section>
            <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
              Dreaming (REM)
            </h3>
            <div className="flex flex-wrap items-center gap-3 px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={memCfg.dreaming_enabled}
                  onChange={(e) => patchMemConfig({ dreaming_enabled: e.target.checked })}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-xs text-[var(--ct-fg)]">Enabled</span>
              </label>
              <select
                value={memCfg.dreaming_agent || ""}
                onChange={(e) => patchMemConfig({ dreaming_agent: e.target.value })}
                className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
              >
                <option value="">Default provider</option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>{p.display_name}</option>
                ))}
              </select>
              {(() => {
                const prov = providers.find((p) => p.id === (memCfg.dreaming_agent || providers[0]?.id));
                const models = prov?.available_models ?? [];
                return models.length > 0 ? (
                  <select
                    value={memCfg.dreaming_model || ""}
                    onChange={(e) => patchMemConfig({ dreaming_model: e.target.value })}
                    className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
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
                    placeholder="Model (haiku, sonnet...)"
                    className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] w-36 focus:border-[var(--ct-accent)] focus:outline-none"
                  />
                );
              })()}
            </div>
          </section>
        )}

        {/* Librarian (file sync) — placeholder */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Librarian
          </h3>
          <div className="px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] text-xs text-[var(--ct-muted-fg)]">
            File sync worker — watches source-tracked memory items for changes on disk and re-indexes automatically. Coming soon.
          </div>
        </section>

        {/* Next scheduled run countdown */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Next Run
          </h3>
          {cronLoading ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">Loading...</div>
          ) : !dreamCron ? (
            <div className="text-xs text-[var(--ct-muted-fg)]">
              No dreaming cron found. Memory dreaming is not configured.
            </div>
          ) : (
            <div className="flex items-center gap-3 px-3 py-2 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)]">
              <div className="flex-1">
                <div className="text-sm text-[var(--ct-fg)] font-mono">
                  {dreamCron.next_run
                    ? relativeTime(dreamCron.next_run)
                    : "Not scheduled"}
                </div>
                {dreamCron.next_run && (
                  <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5">
                    {new Date(dreamCron.next_run).toLocaleString("en-US", {
                      weekday: "short",
                      month: "short",
                      day: "numeric",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </div>
                )}
              </div>
              <span
                className="px-2 py-0.5 rounded text-[10px] font-semibold uppercase"
                style={{
                  background: dreamCron.enabled ? "#10b98122" : "#ef444422",
                  color: dreamCron.enabled ? "#10b981" : "#ef4444",
                }}
              >
                {dreamCron.enabled ? "enabled" : "disabled"}
              </span>
            </div>
          )}
        </section>


        {/* Run history (from workers) */}
        <section>
          <h3 className="text-xs font-semibold text-[var(--ct-muted-fg)] uppercase tracking-wider mb-2">
            Run History
          </h3>
          {dreamWorkers.length === 0 && !cronLoading && (
            <div className="text-xs text-[var(--ct-muted-fg)]">No dream runs yet</div>
          )}
          <div className="space-y-1">
            {dreamWorkers.map(w => {
              const isDone = w.status === "done" || w.status === "error" || w.status === "orphaned";
              const color = w.status === "done" ? "#10b981" : w.status === "error" ? "#ef4444" : "#eab308";
              const dur = isDone && w.done_at ? formatDuration(w.spawned_at, w.done_at) : "";
              return (
                <div
                  key={w.worker_id}
                  className="flex items-center gap-2.5 px-3 py-1.5 rounded bg-[var(--ct-hover-bg)] border border-[var(--ct-border)] cursor-pointer hover:border-[var(--ct-accent)]"
                  onClick={() => setLogWorker(w)}
                >
                  <span
                    className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase w-[58px] text-center shrink-0"
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
    </div>
  );
}

export const MemoryScheduledTab = memo(MemoryScheduledTabImpl);
