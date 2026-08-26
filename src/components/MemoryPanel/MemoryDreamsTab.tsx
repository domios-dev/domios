import { memo, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import {
  Moon,
  Play,
  Loader2,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import type { ApiConfig, CronJob, GraphWorkerInfo } from "../../lib/types";
import { baseUrl, authHeaders } from "./memoryApi";
import { BusySpinner } from "../shared/BusyState";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { DreamCard, DreamDetailModal, type DreamDiaryEntry } from "./DreamCard";
import { hasRenderableDreamInsight } from "./dreamVisuals";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";

interface Props {
  api: ApiConfig;
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

function MemoryDreamsTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [openDream, setOpenDream] = useState<DreamDiaryEntry | null>(null);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [showAllSessions, setShowAllSessions] = useState(false);
  const carouselRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

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
        const entries = (data.hits || [])
          .map((h) => h.item)
          .filter((entry) => hasRenderableDreamInsight(entry.body));
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
        /* listen setup failed — no live updates this session */
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
    } catch (e) {
      if (mountedRef.current) {
        setRunError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      if (mountedRef.current) setRunning(false);
    }
  }, [api, running]);

  const inflight = dreamWorkers.find(
    (w) => !isTerminalWorkerStatus(w.status),
  );

  const scrollCarousel = useCallback((dir: 1 | -1) => {
    const el = carouselRef.current;
    if (!el) return;
    const card = el.querySelector("[data-dream-card]") as HTMLElement | null;
    const step = card ? card.offsetWidth + 16 : el.clientWidth * 0.7;
    el.scrollBy({ left: dir * step, behavior: "smooth" });
  }, []);

  return (
    <div className="absolute inset-0 overflow-y-auto bg-[var(--ct-bg)]" data-testid="memory-dreams-tab">
      <div
        className="pointer-events-none fixed inset-0 -z-0 opacity-25"
        style={{
          background:
            "radial-gradient(circle at 20% 0%, color-mix(in srgb, var(--ct-magenta, #a78bfa) 22%, transparent) 0%, transparent 45%), radial-gradient(circle at 90% 110%, color-mix(in srgb, var(--ct-blue, #60a5fa) 18%, transparent) 0%, transparent 50%)",
        }}
      />

      <div className="relative px-5 pt-5 pb-3 flex items-start gap-4">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 text-[22px] font-light text-[var(--ct-fg)] leading-tight">
            <Moon size={18} className="opacity-70" />
            <span>What memory dreamt.</span>
          </div>
          <div className="mt-1.5 text-[11px] text-[var(--ct-muted-fg)] flex items-center gap-2 flex-wrap">
            {(() => {
              const statusText = inflight
                ? `Dreaming now · ${formatDuration(inflight.spawned_at, Date.now())}`
                : dreamCron?.next_run
                  ? `Next session ${relativeTime(dreamCron.next_run)}`
                  : !dreamCron
                    ? "Not configured"
                    : null;
              return (
                <>
                  {statusText && (
                    <>
                      <span>{statusText}</span>
                      <span className="opacity-30">·</span>
                    </>
                  )}
                  <span>{diaryEntries.length} dream{diaryEntries.length === 1 ? "" : "s"} recorded</span>
                </>
              );
            })()}
          </div>
        </div>

        <div className="shrink-0 flex items-center gap-2">
          {dreamCron && (
            <button
              type="button"
              data-testid="memory-dream-nightly-toggle"
              onClick={() => toggleDreamCron(!dreamCron.enabled)}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-full bg-[var(--ct-bg)]/40 border border-[var(--ct-border)] cursor-pointer hover:border-[var(--ct-accent)] transition-colors"
              aria-label={dreamCron.enabled ? "Disable nightly dreaming" : "Enable nightly dreaming"}
            >
              <span
                className="inline-block w-1.5 h-1.5 rounded-full"
                style={{ background: dreamCron.enabled ? "#34d399" : "var(--ct-muted-fg)" }}
              />
              <span
                className="text-[10px] uppercase tracking-wider"
                style={{ color: dreamCron.enabled ? "#34d399" : "var(--ct-muted-fg)" }}
              >
                {dreamCron.enabled ? "Nightly on" : "Nightly off"}
              </span>
            </button>
          )}
          <button
            data-testid="memory-dream-run-now"
            onClick={runNow}
            disabled={running || !!inflight}
            className="flex items-center gap-2 px-3 py-1.5 rounded-full text-[var(--ct-bg)] text-[11px] font-medium tracking-wide hover:shadow-[0_0_18px_color-mix(in_srgb,_var(--ct-accent)_55%,_transparent)] disabled:opacity-50 disabled:cursor-wait transition-shadow border-0 cursor-pointer"
            style={{
              background:
                "linear-gradient(135deg, color-mix(in srgb, var(--ct-magenta, #a78bfa) 75%, var(--ct-accent)) 0%, var(--ct-accent) 100%)",
              color: "var(--ct-bg)",
            }}
          >
            {running || inflight ? (
              <>
                <BusySpinner icon={Loader2} size={13} />
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
      </div>

      {runError && (
        <div className="mx-5 mb-3 px-3 py-2 rounded-md bg-[#f87171]/15 border border-[#f87171]/40 text-[11px] text-[#fecaca]" data-testid="memory-dream-run-error" role="alert">
          {runError}
        </div>
      )}

      <section className="relative pb-8">
        <div className="flex items-center justify-between px-5 pb-2">
          <h3 className="text-[10px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)]">
            Dream diary
          </h3>
          {diaryEntries.length > 1 && (
            <div className="flex gap-1">
              <button
                onClick={() => scrollCarousel(-1)}
                data-testid="memory-dream-scroll-left"
                className="w-7 h-7 rounded-full border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] flex items-center justify-center bg-transparent cursor-pointer"
                aria-label="Scroll left"
              >
                <ChevronLeft size={14} />
              </button>
              <button
                onClick={() => scrollCarousel(1)}
                data-testid="memory-dream-scroll-right"
                className="w-7 h-7 rounded-full border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-fg)] flex items-center justify-center bg-transparent cursor-pointer"
                aria-label="Scroll right"
              >
                <ChevronRight size={14} />
              </button>
            </div>
          )}
        </div>

        {diaryLoading ? (
          <SkeletonCarousel />
        ) : diaryEntries.length === 0 ? (
          <EmptyDream />
        ) : (
          <div
            ref={carouselRef}
            className="flex gap-4 overflow-x-auto px-5 pb-3 snap-x snap-mandatory scroll-pl-5"
            style={{ scrollbarWidth: "thin" }}
            data-testid="memory-dream-carousel"
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

      <section className="px-5 pb-6" data-testid="memory-dream-sessions">
        <h3 className="text-[10px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-2">
          Recent sessions
        </h3>
        <RecentSessionsList
          workers={dreamWorkers}
          showAll={showAllSessions}
          onToggleAll={() => setShowAllSessions((v) => !v)}
          onSelect={(w) => setLogWorker(w)}
        />
      </section>

      {logWorker &&
        (logWorker.session_id ? (
          <RunChatPopup
            title={logWorker.intent.replace(/^dreaming:/, "dream:")}
            subtitle={[logWorker.agent_provider, logWorker.model].filter(Boolean).join(" / ")}
            status={logWorker.status}
            report={logWorker.report}
            provider={logWorker.agent_provider ?? defaultSpawnProviderId()}
            cwd={logWorker.cwd ?? ""}
            sessionId={logWorker.session_id}
            live={!isTerminalWorkerStatus(logWorker.status)}
            onClose={() => setLogWorker(null)}
          />
        ) : (
          <AgentRunLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />
        ))}
      {openDream && (
        <DreamDetailModal entry={openDream} onClose={() => setOpenDream(null)} />
      )}
    </div>
  );
}

function EmptyDream() {
  return (
    <div className="mx-5 rounded-2xl border border-dashed border-[var(--ct-border)] px-6 py-10 flex flex-col items-center gap-2 text-center" data-testid="memory-dream-empty">
      <Moon size={28} className="text-[var(--ct-magenta,#a78bfa)] opacity-70" />
      <div className="text-[12px] text-[var(--ct-fg)]">No dreams yet</div>
      <div className="text-[10px] text-[var(--ct-muted-fg)] max-w-[300px] leading-relaxed">
        Here you'll find what Rollo dreamt up — his nightly walk through your memory. Trigger a session manually or wait for the nightly run.
      </div>
    </div>
  );
}

function SkeletonDream() {
  return (
    <div
      data-dream-card
      className="snap-start shrink-0 w-[260px] h-[280px] rounded-2xl border border-[var(--ct-border)] bg-[var(--ct-bg-hover)]/30 animate-pulse overflow-hidden flex flex-col"
    >
      <div className="flex items-start justify-between px-4 pt-4">
        <div className="w-9 h-9 rounded-lg bg-[var(--ct-border)]" />
        <div className="flex flex-col items-end gap-1.5">
          <div className="h-6 w-10 rounded bg-[var(--ct-border)]" />
          <div className="h-2 w-12 rounded bg-[var(--ct-border)]/70" />
        </div>
      </div>
      <div className="px-4 mt-3">
        <div className="h-3 w-20 rounded bg-[var(--ct-border)]" />
      </div>
      <div className="flex-1 px-4 pt-3 pb-3 flex flex-col gap-2">
        <div className="h-2.5 w-[90%] rounded bg-[var(--ct-border)]" />
        <div className="h-2.5 w-[80%] rounded bg-[var(--ct-border)]" />
        <div className="h-2.5 w-[85%] rounded bg-[var(--ct-border)]" />
        <div className="h-2.5 w-[60%] rounded bg-[var(--ct-border)]" />
      </div>
      <div className="px-4 pb-3 pt-1 border-t border-white/5 flex items-center gap-2">
        <div className="h-3 w-12 rounded bg-[var(--ct-border)]" />
        <div className="h-3 w-10 rounded bg-[var(--ct-border)] ml-auto" />
      </div>
    </div>
  );
}

function SkeletonCarousel() {
  return (
    <div className="flex gap-4 overflow-x-hidden px-5 pb-3">
      <SkeletonDream />
      <SkeletonDream />
      <SkeletonDream />
    </div>
  );
}

function RecentSessionsList({
  workers,
  showAll,
  onToggleAll,
  onSelect,
}: {
  workers: GraphWorkerInfo[];
  showAll: boolean;
  onToggleAll: () => void;
  onSelect: (w: GraphWorkerInfo) => void;
}) {
  if (workers.length === 0) {
    return <div className="text-[11px] text-[var(--ct-muted-fg)]">No dream sessions yet.</div>;
  }
  const capped = workers.slice(0, 12);
  const first = capped.slice(0, 5);
  const rest = capped.slice(5);
  const renderRow = (w: GraphWorkerInfo) => {
    const isDone = isTerminalWorkerStatus(w.status);
    const color = statusColor(w.status);
    const elapsed =
      isDone && w.done_at
        ? formatDuration(w.spawned_at, w.done_at)
        : formatDuration(w.spawned_at, Date.now());
    return (
      <div
        key={w.worker_id}
        onClick={() => onSelect(w)}
        data-testid="memory-dream-session"
        data-worker-id={w.worker_id}
        data-worker-status={w.status}
        className="flex items-center gap-3 px-3 py-1.5 rounded-md bg-[var(--ct-bg-hover)]/60 border border-[var(--ct-border)] hover:border-[var(--ct-accent)] cursor-pointer transition-colors"
      >
        <span className="text-[10px] uppercase tracking-wider w-[60px] shrink-0" style={{ color }}>
          {isDone ? w.status : "running"}
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
  };
  return (
    <div data-testid="memory-dream-session-list">
      <div className="space-y-1">{first.map(renderRow)}</div>
      {rest.length > 0 && (
        <div className="flex flex-col items-center">
          <button
            data-testid="memory-dream-show-all-sessions"
            onClick={onToggleAll}
            className="text-[10px] uppercase tracking-wider text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] mt-2 cursor-pointer bg-transparent border-0"
          >
            {showAll ? "Show less" : `Show all (${workers.length})`}
          </button>
          {showAll && (
            <div className="w-full space-y-1 mt-2">{rest.map(renderRow)}</div>
          )}
        </div>
      )}
    </div>
  );
}

export const MemoryDreamsTab = memo(MemoryDreamsTabImpl);
