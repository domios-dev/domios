import { memo, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Moon,
  Play,
  Loader2,
  ChevronLeft,
  ChevronRight,
  Sparkles,
} from "lucide-react";
import type { ApiConfig, CronJob, GraphWorkerInfo } from "../../lib/types";
import { canonicalizeShort } from "../../lib/modelLabels";
import { baseUrl, authHeaders, isTauriEnv } from "./memoryApi";
import { WorkerLogPopup } from "./WorkerLogPopup";
import { DreamCard, DreamDetailModal, type DreamDiaryEntry } from "./DreamCard";

interface Props {
  api: ApiConfig;
}

interface SearchHit {
  item: DreamDiaryEntry;
  score: number;
}

interface MemoryConfig {
  dreaming_agent: string;
  dreaming_model: string;
  file_sync_enabled: boolean;
  file_sync_interval_minutes: number;
}

interface ProviderInfo {
  id: string;
  display_name: string;
  available_models?: Array<{ id: string; display_name: string }>;
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

function statusColor(status: string): string {
  switch (status) {
    case "done":
    case "Success":
      return "#34d399";
    case "error":
    case "Failed":
      return "#f87171";
    case "Cancelled":
    case "orphaned":
      return "#9ca3af";
    default:
      return "#fbbf24";
  }
}

function scheduleLabel(schedule: string): string {
  if (schedule.includes("0 3 * * *") || schedule.includes("0 2 * * *") || schedule.includes("0 4 * * *")) {
    return "Every night ~3 AM";
  }
  if (schedule.includes("0 * * * *")) return "Every hour";
  return schedule;
}

function MemoryDreamsTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [openDream, setOpenDream] = useState<DreamDiaryEntry | null>(null);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const [memCfg, setMemCfg] = useState<MemoryConfig | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [defaultProvider, setDefaultProvider] = useState<string>("claude");
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const carouselRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    fetch(`${baseUrl(api)}/api/config?key=memory`, { headers: authHeaders(api) })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => { if (d?.value && mountedRef.current) setMemCfg(d.value); })
      .catch(() => {});
    fetch(`${baseUrl(api)}/api/config?key=general_agent`, { headers: authHeaders(api) })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        const p = d?.value?.agent_provider;
        if (p && mountedRef.current) setDefaultProvider(p);
      })
      .catch(() => {});
    fetch(`${baseUrl(api)}/api/agents/providers`, { headers: authHeaders(api) })
      .then((r) => (r.ok ? r.json() : []))
      .then((p) => { if (mountedRef.current) setProviders(p); })
      .catch(() => {});
  }, [api]);

  const patchMemConfig = useCallback(
    (patch: Partial<MemoryConfig>) => {
      if (!memCfg) return;
      const next = { ...memCfg, ...patch };
      setMemCfg(next);
      fetch(`${baseUrl(api)}/api/config`, {
        method: "PATCH",
        headers: { ...authHeaders(api), "Content-Type": "application/json" },
        body: JSON.stringify({ key: "memory", value: next }),
      }).catch(() => {});
    },
    [api, memCfg],
  );

  const toggleDreamCron = useCallback(
    async (enable: boolean) => {
      if (!dreamCron) return;
      const endpoint = enable ? "enable" : "disable";
      try {
        const res = await fetch(
          `${baseUrl(api)}/api/crons/${encodeURIComponent(dreamCron.id)}/${endpoint}`,
          { method: "POST", headers: authHeaders(api) },
        );
        if (res.ok && mountedRef.current) {
          setDreamCron((await res.json()) as CronJob);
        }
      } catch {
        /* network */
      }
    },
    [api, dreamCron],
  );

  const fetchCronData = useCallback(async () => {
    try {
      const res = await fetch(`${baseUrl(api)}/api/crons`, { headers: authHeaders(api) });
      if (res.ok && mountedRef.current) {
        const jobs = (await res.json()) as CronJob[];
        const dream = jobs.find((j) => j.name === "mem-dreaming-auto") ?? null;
        setDreamCron(dream);
      }
      const wRes = await fetch(`${baseUrl(api)}/api/mem/workers`, { headers: authHeaders(api) });
      if (wRes.ok && mountedRef.current) {
        const workers = (await wRes.json()) as GraphWorkerInfo[];
        const dreamRuns = workers
          .filter((w) => w.intent.startsWith("dreaming:"))
          .sort((a, b) => b.spawned_at - a.spawned_at);
        setDreamWorkers(dreamRuns);
      }
    } catch {
      /* network */
    }
  }, [api]);

  const fetchDiary = useCallback(
    async (showLoading = false) => {
      if (showLoading) setDiaryLoading(true);
      try {
        const params = new URLSearchParams({ q: "dream", kind: "daily", limit: "30" });
        const res = await fetch(`${baseUrl(api)}/api/mem/search?${params}`, {
          headers: authHeaders(api),
        });
        if (!res.ok || !mountedRef.current) return;
        const data = (await res.json()) as { hits: SearchHit[] };
        const entries = (data.hits || []).map((h) => h.item);
        entries.sort(
          (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
        );
        setDiaryEntries(entries);
      } catch {
        /* network */
      } finally {
        if (mountedRef.current && showLoading) setDiaryLoading(false);
      }
    },
    [api],
  );

  useEffect(() => { fetchCronData(); }, [fetchCronData]);
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
      workerTimer = setTimeout(() => { if (active) fetchCronData(); }, 500);
    };
    const refreshDiary = () => {
      if (!active) return;
      if (diaryTimer) clearTimeout(diaryTimer);
      diaryTimer = setTimeout(() => { if (active) fetchDiary(false); }, 500);
    };
    (async () => {
      try {
        for (const name of [
          "graph-worker-spawned",
          "graph-worker-progress",
          "graph-worker-reported",
          "graph-worker-done",
        ]) {
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
        const cronFn = await listen("cron-run-finished", refreshWorkers);
        if (!active) { cronFn(); return; }
        unlisteners.push(cronFn);
      } catch {
        /* not in tauri */
      }
    })();
    return () => {
      active = false;
      if (workerTimer) clearTimeout(workerTimer);
      if (diaryTimer) clearTimeout(diaryTimer);
      unlisteners.forEach((fn) => fn());
    };
  }, [fetchCronData, fetchDiary]);

  const runNow = useCallback(async () => {
    if (running) return;
    setRunning(true);
    setRunError(null);
    try {
      const res = await fetch(`${baseUrl(api)}/api/mem/dream`, {
        method: "POST",
        headers: { ...authHeaders(api), "Content-Type": "application/json" },
        body: JSON.stringify({ phase: "all" }),
      });
      if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
      // Worker spawned — server response just confirms start; live updates
      // will arrive via the listener.
    } catch (e) {
      if (mountedRef.current) {
        setRunError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (mountedRef.current) setRunning(false);
    }
  }, [api, running]);

  const inflight = dreamWorkers.find(
    (w) => w.status !== "done" && w.status !== "error" && w.status !== "orphaned",
  );

  const scrollCarousel = useCallback((dir: 1 | -1) => {
    const el = carouselRef.current;
    if (!el) return;
    const card = el.querySelector("[data-dream-card]") as HTMLElement | null;
    const step = card ? card.offsetWidth + 16 : el.clientWidth * 0.7;
    el.scrollBy({ left: dir * step, behavior: "smooth" });
  }, []);

  const displayDreamingModel = canonicalizeShort(memCfg?.dreaming_model || "");

  return (
    <div className="absolute inset-0 overflow-y-auto bg-[var(--ct-bg)]">
      {/* Atmospheric backdrop — pulled from the live theme so it always
          tints with the user's chosen palette instead of fixed indigo. */}
      <div
        className="pointer-events-none fixed inset-0 -z-0 opacity-25"
        style={{
          background:
            "radial-gradient(circle at 20% 0%, color-mix(in srgb, var(--ct-magenta, #a78bfa) 22%, transparent) 0%, transparent 45%), radial-gradient(circle at 90% 110%, color-mix(in srgb, var(--ct-blue, #60a5fa) 18%, transparent) 0%, transparent 50%)",
        }}
      />

      <div className="relative px-5 pt-5 pb-3 flex items-start gap-4">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.25em] text-[var(--ct-muted-fg)]">
            <Moon size={12} className="opacity-70" />
            <span>The dreamer</span>
          </div>
          <div className="mt-1 text-[22px] font-light text-[var(--ct-fg)] leading-tight">
            What memory dreamt.
          </div>
          <div className="mt-1.5 text-[11px] text-[var(--ct-muted-fg)] flex items-center gap-2 flex-wrap">
            <span>
              {inflight
                ? `Dreaming now · ${formatDuration(inflight.spawned_at, Date.now())}`
                : dreamCron?.next_run
                  ? `Next session ${relativeTime(dreamCron.next_run)}`
                  : dreamCron
                    ? "Manual trigger only"
                    : "Not configured"}
            </span>
            <span className="opacity-30">·</span>
            <span>{diaryEntries.length} dream{diaryEntries.length === 1 ? "" : "s"} recorded</span>
          </div>
        </div>

        <button
          onClick={runNow}
          disabled={running || !!inflight}
          className="shrink-0 flex items-center gap-2 px-3 py-1.5 rounded-full text-[var(--ct-bg)] text-[11px] font-medium tracking-wide hover:shadow-[0_0_18px_color-mix(in_srgb,_var(--ct-accent)_55%,_transparent)] disabled:opacity-50 disabled:cursor-wait transition-shadow border-0 cursor-pointer"
          style={{
            background:
              "linear-gradient(135deg, color-mix(in srgb, var(--ct-magenta, #a78bfa) 75%, var(--ct-accent)) 0%, var(--ct-accent) 100%)",
            color: "var(--ct-bg)",
          }}
        >
          {running || inflight ? (
            <>
              <Loader2 size={13} className="animate-spin" />
              {inflight ? "Dreaming…" : "Starting…"}
            </>
          ) : (
            <>
              <Play size={12} fill="white" />
              Dream now
            </>
          )}
        </button>
      </div>

      {runError && (
        <div className="mx-5 mb-3 px-3 py-2 rounded-md bg-[#f87171]/15 border border-[#f87171]/40 text-[11px] text-[#fecaca]">
          {runError}
        </div>
      )}

      {/* Dream carousel */}
      <section className="relative pb-8">
        <div className="flex items-center justify-between px-5 pb-2">
          <h3 className="text-[10px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)]">
            Dream diary
          </h3>
          {diaryEntries.length > 1 && (
            <div className="flex gap-1">
              <button
                onClick={() => scrollCarousel(-1)}
                className="w-7 h-7 rounded-full border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] flex items-center justify-center bg-transparent cursor-pointer"
                aria-label="Scroll left"
              >
                <ChevronLeft size={14} />
              </button>
              <button
                onClick={() => scrollCarousel(1)}
                className="w-7 h-7 rounded-full border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] flex items-center justify-center bg-transparent cursor-pointer"
                aria-label="Scroll right"
              >
                <ChevronRight size={14} />
              </button>
            </div>
          )}
        </div>

        {diaryLoading ? (
          <div className="px-5 text-[11px] text-[var(--ct-muted-fg)]">Loading the dreamer's notebook…</div>
        ) : diaryEntries.length === 0 ? (
          <EmptyDream />
        ) : (
          <div
            ref={carouselRef}
            className="flex gap-4 overflow-x-auto px-5 pb-3 snap-x snap-mandatory scroll-pl-5"
            style={{ scrollbarWidth: "thin" }}
          >
            {diaryEntries.map((entry) => (
              <DreamCard
                key={entry.id}
                entry={entry}
                onOpen={() => setOpenDream(entry)}
              />
            ))}
          </div>
        )}
      </section>

      {/* Run history */}
      <section className="px-5 pb-6">
        <h3 className="text-[10px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-2">
          Recent sessions
        </h3>
        {dreamWorkers.length === 0 ? (
          <div className="text-[11px] text-[var(--ct-muted-fg)]">No dream sessions yet.</div>
        ) : (
          <div className="space-y-1">
            {dreamWorkers.slice(0, 12).map((w) => {
              const isDone =
                w.status === "done" || w.status === "error" || w.status === "orphaned";
              const color = statusColor(w.status);
              const elapsed =
                isDone && w.done_at
                  ? formatDuration(w.spawned_at, w.done_at)
                  : formatDuration(w.spawned_at, Date.now());
              return (
                <div
                  key={w.worker_id}
                  onClick={() => setLogWorker(w)}
                  className="flex items-center gap-3 px-3 py-1.5 rounded-md bg-[var(--ct-hover-bg)]/60 border border-[var(--ct-border)] hover:border-[var(--ct-accent)] cursor-pointer transition-colors"
                >
                  <span
                    className="w-1.5 h-1.5 rounded-full shrink-0"
                    style={{
                      background: color,
                      boxShadow: !isDone ? `0 0 8px ${color}` : undefined,
                    }}
                  />
                  <span className="text-[10px] uppercase tracking-wider w-[60px] shrink-0" style={{ color }}>
                    {w.status}
                  </span>
                  <span className="text-[11px] text-[var(--ct-fg)] font-mono tabular-nums">
                    {new Date(w.spawned_at).toLocaleString("en-US", {
                      month: "short",
                      day: "numeric",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </span>
                  {w.model && (
                    <span className="text-[9px] text-[var(--ct-muted-fg)] truncate max-w-[120px]">
                      {w.model}
                    </span>
                  )}
                  <span className="ml-auto text-[10px] tabular-nums text-[var(--ct-muted-fg)]">
                    {elapsed}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* Dreamer settings */}
      <section className="px-5 pb-8">
        <h3 className="text-[10px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-2">
          Dreamer settings
        </h3>
        <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 backdrop-blur-sm overflow-hidden">
          <div className="flex items-center gap-3 px-4 py-3 border-b border-[var(--ct-border)]">
            <Sparkles size={14} className="text-[#a78bfa]" />
            <div className="flex-1 min-w-0">
              <div className="text-[12px] font-medium text-[var(--ct-fg)]">Nightly dreaming (REM)</div>
              <div className="text-[10px] text-[var(--ct-muted-fg)] truncate">
                {dreamCron ? scheduleLabel(dreamCron.schedule) : "Not configured"}
              </div>
            </div>
            {dreamCron && (
              <DreamSwitch
                checked={dreamCron.enabled}
                onChange={(v) => toggleDreamCron(v)}
              />
            )}
          </div>
          {memCfg && (
            <div className="px-4 py-3 flex flex-wrap items-center gap-2">
              <label className="text-[10px] text-[var(--ct-muted-fg)] uppercase tracking-wider">Provider</label>
              <select
                value={memCfg.dreaming_agent || ""}
                onChange={(e) => patchMemConfig({ dreaming_agent: e.target.value })}
                className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
              >
                <option value="">
                  Default ({providers.find((p) => p.id === defaultProvider)?.display_name ?? defaultProvider})
                </option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.display_name}
                  </option>
                ))}
              </select>
              <label className="text-[10px] text-[var(--ct-muted-fg)] uppercase tracking-wider ml-2">Model</label>
              {(() => {
                const resolvedProviderId = memCfg.dreaming_agent || defaultProvider;
                const prov = providers.find((p) => p.id === resolvedProviderId);
                const models = prov?.available_models ?? [];
                return models.length > 0 ? (
                  <select
                    value={displayDreamingModel}
                    onChange={(e) => patchMemConfig({ dreaming_model: e.target.value })}
                    className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
                  >
                    <option value="">Default model</option>
                    {models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.display_name}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    value={displayDreamingModel}
                    onChange={(e) => patchMemConfig({ dreaming_model: e.target.value })}
                    placeholder="Model"
                    className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] w-32 focus:border-[var(--ct-accent)] focus:outline-none"
                  />
                );
              })()}
            </div>
          )}
        </div>
      </section>

      {logWorker && (
        <WorkerLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />
      )}
      {openDream && (
        <DreamDetailModal entry={openDream} onClose={() => setOpenDream(null)} />
      )}
    </div>
  );
}

function DreamSwitch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-all border-0 cursor-pointer ${
        checked
          ? "bg-gradient-to-r from-[#7c3aed] to-[#3b82f6] shadow-[0_0_12px_rgba(124,58,237,0.5)]"
          : "bg-[var(--ct-border)]"
      }`}
    >
      <span
        className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
          checked ? "translate-x-4" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

function EmptyDream() {
  return (
    <div className="mx-5 rounded-2xl border border-dashed border-[var(--ct-border)] px-6 py-10 flex flex-col items-center gap-2 text-center">
      <Moon size={28} className="text-[var(--ct-magenta,#a78bfa)] opacity-70" />
      <div className="text-[12px] text-[var(--ct-fg)]">No dreams yet</div>
      <div className="text-[10px] text-[var(--ct-muted-fg)] max-w-[300px] leading-relaxed">
        The dreamer hasn't been to bed yet. Trigger a session manually or wait for the nightly run.
      </div>
    </div>
  );
}

export const MemoryDreamsTab = memo(MemoryDreamsTabImpl);
