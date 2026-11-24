import { InlineErrorNotice } from "../shared/ErrorNotice";
import { daemonFetch } from "../../lib/webApi";
import { memo, useCallback, useEffect, useRef, useState } from "react";
import { formatDuration, formatUntil } from "../../lib/timeLabels";
import { listen } from "../../lib/bridge";
import { Moon, Play } from "lucide-react";
import type { ApiConfig, CronJob, GraphWorkerInfo } from "../../lib/types";
import { authHeaders } from "./memoryApi";
import { BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";
import { RunChatPopup } from "../AgentRun/RunChatPopup";
import { defaultSpawnProviderId } from "../../lib/agentDescriptorCache";
import { DreamCard, DreamDetailModal, type DreamDiaryEntry } from "./DreamCard";
import { hasRenderableDreamInsight } from "./dreamVisuals";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { workerTimeLabels } from "./workerActivityModel";
import { diaryRecordState, dreamRunLabel, dreamRunReport } from "./dreamRunStatus";

interface Props {
  api: ApiConfig;
}

const diaryGridClass = "grid grid-cols-[repeat(auto-fill,minmax(min(100%,290px),1fr))] gap-3 px-4 @lg:px-7";

function MemoryDreamsTabImpl({ api }: Props) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);
  const [openDream, setOpenDream] = useState<DreamDiaryEntry | null>(null);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryTotal, setDiaryTotal] = useState(0);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [showAllSessions, setShowAllSessions] = useState(false);
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
        const res = await daemonFetch(api,
          `/api/crons/${encodeURIComponent(dreamCron.id)}/${endpoint}`,
          { method: "POST", headers: authHeaders(api) },
        );
        if (res.ok && mountedRef.current) {
          setDreamCron((await res.json()) as CronJob);
        }
      } catch {
        /* network */
      }
    },
    [api.host, api.port, api.token, dreamCron],
  );

  const fetchCronData = useCallback(async () => {
    try {
      const res = await daemonFetch(api, `/api/crons`, { headers: authHeaders(api) });
      if (res.ok && mountedRef.current) {
        const jobs = (await res.json()) as CronJob[];
        const dream = jobs.find((j) => j.name === "mem-dreaming-auto") ?? null;
        setDreamCron(dream);
      }
      const wRes = await daemonFetch(api, `/api/mem/workers`, { headers: authHeaders(api) });
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
  }, [api.host, api.port, api.token]);

  const fetchDiary = useCallback(
    async (showLoading = false) => {
      if (showLoading) setDiaryLoading(true);
      try {
        const params = new URLSearchParams({ limit: "30" });
        const res = await daemonFetch(api, `/api/mem/dream/diary?${params}`, {
          headers: authHeaders(api),
        });
        if (!res.ok || !mountedRef.current) return;
        const data = (await res.json()) as { items: DreamDiaryEntry[]; total: number };
        const entries = (data.items || [])
          .filter((entry) => hasRenderableDreamInsight(entry.body));
        setDiaryEntries(entries);
        setDiaryTotal(data.total);
      } catch {
        /* network */
      } finally {
        if (mountedRef.current && showLoading) setDiaryLoading(false);
      }
    },
    [api.host, api.port, api.token],
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
      const res = await daemonFetch(api, `/api/mem/dream`, {
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
  }, [api.host, api.port, api.token, running]);

  const inflight = dreamWorkers.find(
    (w) => !isTerminalWorkerStatus(w.status),
  );

  const diaryStates = diaryEntries.map((entry) =>
    diaryRecordState(entry.id, entry.body, dreamWorkers),
  );
  // Incompleteness is judgeable only from bodies this page loaded, so it flags what
  // is shown and never becomes a second count competing with diaryTotal.
  const unfinishedInView = diaryStates.some((s) => s.state === "incomplete_recoverable");

  return (
    <div className="@container absolute inset-0 overflow-y-auto bg-[var(--ct-bg)]" data-testid="memory-dreams-tab">
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-2 gap-y-4 px-4 pt-5 pb-7 @lg:grid-cols-[minmax(0,1fr)_auto_auto] @lg:gap-x-3 @lg:px-7 @lg:pt-6">
        <div>
          <h2 className="flex items-center gap-2.5 text-xl leading-tight font-bold text-[var(--ct-fg)]">
            <Moon size={18} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
            <span>What memory dreamt</span>
          </h2>
          <div
            className="mt-1.5 flex flex-wrap items-center gap-x-1.5 text-xs text-[var(--ct-muted-fg)]"
            data-testid="memory-dream-meta"
          >
            {(() => {
              const statusText = inflight
                ? `Dreaming now · ${formatDuration(Date.now() - inflight.spawned_at)}`
                : dreamCron?.next_run
                  ? `Next session ${formatUntil(dreamCron.next_run, Date.now())}`
                  : !dreamCron
                    ? "Not configured"
                    : null;
              return (
                <>
                  <span>{diaryTotal} dream{diaryTotal === 1 ? "" : "s"} recorded</span>
                  {statusText && (
                    <span className="inline-flex items-baseline gap-1.5">
                      <span aria-hidden="true">·</span>
                      <span>{statusText}</span>
                    </span>
                  )}
                  {unfinishedInView && (
                    <span className="inline-flex items-baseline gap-1.5">
                      <span className="opacity-30" aria-hidden="true">·</span>
                      <span
                        data-testid="memory-dream-unfinished-flag"
                        style={{ color: "var(--ct-yellow)" }}
                      >
                        unfinished diaries in view need recovery
                      </span>
                    </span>
                  )}
                </>
              );
            })()}
          </div>
        </div>

        {dreamCron && (
          <Button
            variant="ghost"
            size="sm"
            type="button"
            data-testid="memory-dream-nightly-toggle"
            className="col-start-2 row-start-1 gap-1.5 whitespace-nowrap"
            onClick={() => toggleDreamCron(!dreamCron.enabled)}
            aria-label={dreamCron.enabled ? "Disable nightly dreaming" : "Enable nightly dreaming"}
          >
            <span
              className="inline-block w-1.5 h-1.5 rounded-full"
              style={{ background: dreamCron.enabled ? "var(--ct-green)" : "var(--ct-muted-fg)" }}
            />
            <span className="text-xs text-[var(--ct-muted-fg)]">
              {dreamCron.enabled ? "Nightly on" : "Nightly off"}
            </span>
          </Button>
        )}
        <Button
          variant="accent"
          data-testid="memory-dream-run-now"
          className="col-start-1 row-start-2 h-8! min-h-8! justify-self-start gap-2 rounded-[9px]! border-[color-mix(in_srgb,var(--ct-accent)_36%,transparent)]! px-3.5! py-0! text-[13px]! font-semibold! @lg:col-start-3 @lg:row-start-1"
          onClick={runNow}
          disabled={running || !!inflight}
        >
          {running || inflight ? (
            <>
              <BusyState inline hideLabel label={inflight ? "Dreaming" : "Starting"} phase="work" dotSize="micro" />
              {inflight ? "Dreaming…" : "Starting…"}
            </>
          ) : (
            <>
              <Play size={13} aria-hidden="true" />
              Dream now
            </>
          )}
        </Button>
      </div>

      {runError && (
        <div data-testid="memory-dream-run-error"><InlineErrorNotice error={runError} operation="running memory maintenance" blocking /></div>
      )}

      <section className="pb-7">
        <div className="flex items-baseline gap-2 px-4 pb-3 text-xs @lg:px-7">
          <h3 className="font-semibold text-[var(--ct-fg)]">Dream diary</h3>
          {!!diaryEntries.length && (
            <span className="text-[var(--ct-muted-fg)]">· newest first</span>
          )}
        </div>

        {diaryLoading ? (
          <SkeletonDiary />
        ) : diaryEntries.length === 0 ? (
          <EmptyDream />
        ) : (
          <div className={diaryGridClass} data-testid="memory-dream-grid">
            {diaryEntries.map((entry, index) => (
              <DreamCard
                key={entry.id}
                entry={entry}
                recordState={diaryStates[index]}
                newest={index === 0}
                onOpen={() => setOpenDream(entry)}
              />
            ))}
          </div>
        )}
      </section>

      <section className="px-4 pb-6 @lg:px-7" data-testid="memory-dream-sessions">
        <h3 className="mb-3 text-xs font-semibold text-[var(--ct-fg)]">
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
          <RunChatPopup tabId={logWorker.tab_id}
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
        <DreamDetailModal
          entry={openDream}
          report={dreamRunReport(openDream.id, dreamWorkers)}
          onClose={() => setOpenDream(null)}
        />
      )}
    </div>
  );
}

function EmptyDream() {
  return (
    <div className="ct-card ct-card--static mx-4 px-6 py-10 flex flex-col items-center gap-2 text-center @lg:mx-7" data-testid="memory-dream-empty">
      <Moon size={28} className="text-[var(--ct-accent)] opacity-70" />
      <div className="text-sm font-medium text-[var(--ct-fg)]">No dreams yet</div>
      <div className="text-xs text-[var(--ct-muted-fg)] max-w-[300px] leading-relaxed">
        Start a session or wait for the nightly run to see what memory dreamt.
      </div>
    </div>
  );
}

function SkeletonDream() {
  return (
    <div className="ct-card ct-card--static flex animate-pulse flex-col gap-3 px-[17px] py-[15px]">
      <div className="flex items-center gap-2">
        <div className="ct-card__icon bg-[var(--ct-border)]" />
        <div className="h-3 w-28 rounded bg-[var(--ct-border)]" />
      </div>
      <div className="h-4 w-[85%] rounded bg-[var(--ct-border)]" />
      <div className="flex flex-col gap-2">
        <div className="h-2.5 w-[90%] rounded bg-[var(--ct-border)]/70" />
        <div className="h-2.5 w-[80%] rounded bg-[var(--ct-border)]/70" />
        <div className="h-2.5 w-[60%] rounded bg-[var(--ct-border)]/70" />
      </div>
    </div>
  );
}

function SkeletonDiary() {
  return (
    <div className={diaryGridClass} aria-hidden="true">
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
    const run = dreamRunLabel(w);
    const time = workerTimeLabels(w, Date.now());
    return (
      <div
        key={w.worker_id}
        onClick={() => onSelect(w)}
        data-testid="memory-dream-session"
        data-worker-id={w.worker_id}
        data-worker-status={w.status}
        data-run-label={run.label}
        title={run.reason ?? undefined}
        className="grid cursor-pointer grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-2 gap-y-1 px-3.5 py-2.5 text-xs hover:bg-[var(--ct-hover)] @lg:grid-cols-[13ch_23ch_minmax(0,1fr)]"
      >
        <span
          className="flex items-center gap-1.5 whitespace-nowrap @lg:whitespace-normal"
          style={{ color: run.color }}
        >
          <span aria-hidden="true" className="size-1.5 shrink-0 rounded-full bg-current" />
          <span>{w.status === "orphaned" ? `orphaned · ${run.label}` : run.label}</span>
        </span>
        <span className="text-[var(--ct-fg)] tabular-nums">
          {new Date(w.spawned_at).toLocaleString("en-US", {
            month: "short",
            day: "numeric",
            hour: "2-digit",
            minute: "2-digit",
          })}
        </span>
        <div className="col-span-full flex min-w-0 flex-wrap items-baseline gap-x-1 gap-y-0.5 text-[var(--ct-muted-fg)] @lg:col-start-3 @lg:col-end-4 @lg:row-start-1 @lg:flex-nowrap @lg:gap-x-3">
          {w.model && (
            <>
              <span className="min-w-0 wrap-anywhere @lg:truncate @lg:font-mono" title={w.model}>{w.model}</span>
              <span aria-hidden="true" className="@lg:hidden">·</span>
            </>
          )}
          <span className="shrink-0 tabular-nums @lg:ml-auto">
            {time.age} · {time.duration}
          </span>
        </div>
        {run.reason && (
          <div className="col-span-full" data-testid="memory-dream-session-reason">
            <InlineErrorNotice error={{ code: "memory_run_incomplete", raw: run.reason }} />
          </div>
        )}
      </div>
    );
  };
  return (
    <div data-testid="memory-dream-session-list">
      <div className="overflow-hidden rounded-xl border border-[var(--ct-border)] divide-y divide-[var(--ct-border)]">
        {first.map(renderRow)}
      </div>
      {rest.length > 0 && (
        <div className="flex flex-col items-center">
          <Button
            size="sm"
            data-testid="memory-dream-show-all-sessions"
            onClick={onToggleAll}
            className="mt-2"
          >
            {showAll ? "Show less" : `Show all (${workers.length})`}
          </Button>
          {showAll && (
            <div className="mt-2 w-full overflow-hidden rounded-xl border border-[var(--ct-border)] divide-y divide-[var(--ct-border)]">
              {rest.map(renderRow)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export const MemoryDreamsTab = memo(MemoryDreamsTabImpl);
