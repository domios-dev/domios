import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity as ActivityIcon,
  PlusCircle,
  Cpu,
  Library,
  Files,
  RefreshCw,
} from "lucide-react";
import type { ApiConfig, GraphWorkerInfo, GraphWorkerStatus } from "../../lib/types";
import type { MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import type { MemActivityEvent } from "./useActivityPulses";
import { colorForSource } from "./useActivityPulses";
import { baseUrl, authHeaders, isTauriEnv } from "./memoryApi";
import { useGraphWorkers } from "./useGraphWorkers";
import { MemorySpawnWorkerDialog } from "./MemorySpawnWorkerDialog";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

const OP_COLORS: Record<string, string> = {
  write: "#34d399",
  read: "#60a5fa",
  search: "#a78bfa",
  update: "#fbbf24",
  delete: "#f87171",
  recall: "#22d3ee",
  dream: "#c084fc",
};

const STATUS_COLORS: Record<GraphWorkerStatus, string> = {
  starting: "#9ca3af",
  reading: "#60a5fa",
  analyzing: "#60a5fa",
  writing: "#fbbf24",
  done: "#34d399",
  error: "#f87171",
  orphaned: "#6b7280",
};

const MAX_EVENTS = 500;

interface MemoryConfig {
  dreaming_agent: string;
  dreaming_model: string;
  file_sync_enabled: boolean;
  file_sync_interval_minutes: number;
}

interface SyncStatus {
  tracked_files: number;
  stale_files: number;
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function dayLabel(ts: number): string {
  const now = new Date();
  const d = new Date(ts);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const eventDay = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const diff = (today.getTime() - eventDay.getTime()) / 86400000;
  if (diff < 1) return "TODAY";
  if (diff < 2) return "YESTERDAY";
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function MemoryActivityTabImpl({ api, nodes, onNavigateToItem }: Props) {
  const [events, setEvents] = useState<MemActivityEvent[]>([]);
  const [opFilter, setOpFilter] = useState<string>("");
  const [autoScroll, setAutoScroll] = useState(true);
  const [isLive, setIsLive] = useState(false);
  const eventsScrollRef = useRef<HTMLDivElement>(null);
  const newCountRef = useRef(0);
  const [newCount, setNewCount] = useState(0);

  const [memCfg, setMemCfg] = useState<MemoryConfig | null>(null);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const mountedRef = useRef(true);

  const { workers, loading: workersLoading, spawn, kill, refresh: refreshWorkers } = useGraphWorkers(api);
  const [spawnOpen, setSpawnOpen] = useState(false);
  const [logWorker, setLogWorker] = useState<GraphWorkerInfo | null>(null);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    fetch(`${baseUrl(api)}/api/mem/activity/recent?since_ms=0`, { headers: authHeaders(api) })
      .then((r) => r.json())
      .then((data) => {
        if (!active) return;
        const evts = (data.events || []) as MemActivityEvent[];
        evts.sort((a, b) => b.ts_ms - a.ts_ms);
        setEvents(evts.slice(0, MAX_EVENTS));
      })
      .catch(() => {});
    return () => { active = false; };
  }, [api]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    if (isTauriEnv()) {
      listen<MemActivityEvent>("mem-activity", (ev) => {
        if (!active) return;
        setEvents((prev) => {
          const next = [ev.payload, ...prev];
          if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
          return next;
        });
        if (!autoScroll) {
          newCountRef.current++;
          setNewCount(newCountRef.current);
        }
      })
        .then((fn) => {
          if (!active) { fn(); return; }
          unlisten = fn;
          setIsLive(true);
        })
        .catch(() => {});
    }
    return () => { active = false; if (unlisten) unlisten(); };
  }, [api, autoScroll]);

  const fetchMeta = useCallback(() => {
    fetch(`${baseUrl(api)}/api/config?key=memory`, { headers: authHeaders(api) })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => { if (d?.value && mountedRef.current) setMemCfg(d.value); })
      .catch(() => {});
    fetch(`${baseUrl(api)}/api/mem/sync/status`, { headers: authHeaders(api) })
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => { if (d && mountedRef.current) setSyncStatus(d); })
      .catch(() => {});
  }, [api]);
  useEffect(() => { fetchMeta(); }, [fetchMeta]);

  const toggleSync = useCallback(
    (enabled: boolean) => {
      if (!memCfg) return;
      const next = { ...memCfg, file_sync_enabled: enabled };
      setMemCfg(next);
      fetch(`${baseUrl(api)}/api/config`, {
        method: "PATCH",
        headers: { ...authHeaders(api), "Content-Type": "application/json" },
        body: JSON.stringify({ key: "memory", value: next }),
      }).catch(() => {});
    },
    [api, memCfg],
  );

  const filtered = opFilter ? events.filter((e) => e.op === opFilter) : events;

  const scrollToTop = useCallback(() => {
    eventsScrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
    setAutoScroll(true);
    newCountRef.current = 0;
    setNewCount(0);
  }, []);

  const handleScroll = useCallback(() => {
    const el = eventsScrollRef.current;
    if (!el) return;
    if (el.scrollTop > 50 && autoScroll) setAutoScroll(false);
    if (el.scrollTop < 10 && !autoScroll) {
      setAutoScroll(true);
      newCountRef.current = 0;
      setNewCount(0);
    }
  }, [autoScroll]);

  const live = useMemo(() => workers.filter((w) => w.status !== "done" && w.status !== "error" && w.status !== "orphaned"), [workers]);
  const recent = useMemo(() => workers.filter((w) => w.status === "done" || w.status === "error" || w.status === "orphaned").slice(0, 20), [workers]);

  const handleKill = useCallback(
    async (workerId: string) => {
      try {
        await kill(workerId);
        await refreshWorkers();
      } catch (e) {
        window.alert(`Failed to kill: ${e instanceof Error ? e.message : e}`);
      }
    },
    [kill, refreshWorkers],
  );

  let lastDay = "";

  return (
    <div className="absolute inset-0 flex flex-col">
      {/* Header / global filters */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-[var(--ct-border)]">
        <ActivityIcon size={14} className="text-[var(--ct-muted-fg)]" />
        <span className="text-xs font-semibold text-[var(--ct-fg)]">Activity</span>
        <span className="text-[10px] text-[var(--ct-muted-fg)]">
          live signals from memory + workers
        </span>
        <span className="ml-auto flex items-center gap-1 text-[10px] text-[var(--ct-muted-fg)]">
          <span
            className={`w-1.5 h-1.5 rounded-full ${isLive ? "bg-[#34d399] animate-pulse" : "bg-[#fbbf24]"}`}
          />
          {isLive ? "live" : "polling"}
        </span>
      </div>

      {/* 2-column body: events | workers (each scrolls independently) */}
      <div className="flex-1 min-h-0 grid grid-cols-1 md:grid-cols-2 divide-x divide-[var(--ct-border)]">
        {/* Events column */}
        <div className="flex flex-col min-h-0">
          <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--ct-border)]">
            <span className="text-[10px] uppercase tracking-[0.2em] text-[var(--ct-muted-fg)]">Stream</span>
            <select
              className="bg-transparent border border-[var(--ct-border)] rounded px-2 py-0.5 text-[10px] text-[var(--ct-fg)] cursor-pointer ml-2"
              value={opFilter}
              onChange={(e) => setOpFilter(e.target.value)}
            >
              <option value="">All ops</option>
              {Object.keys(OP_COLORS).map((op) => (
                <option key={op} value={op}>
                  {op}
                </option>
              ))}
            </select>
            {opFilter && (
              <button
                onClick={() => setOpFilter("")}
                className="text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] cursor-pointer bg-transparent border-0"
              >
                clear
              </button>
            )}
            <span className="ml-auto text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
              {filtered.length}
            </span>
            <label className="flex items-center gap-1 cursor-pointer text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)]">
              <input
                type="checkbox"
                checked={autoScroll}
                onChange={(e) => setAutoScroll(e.target.checked)}
                className="w-3 h-3"
              />
              follow
            </label>
          </div>

          <div ref={eventsScrollRef} className="flex-1 overflow-y-auto" onScroll={handleScroll}>
            {newCount > 0 && !autoScroll && (
              <div className="sticky top-0 z-10 flex justify-center py-1">
                <button
                  onClick={scrollToTop}
                  className="px-3 py-1 rounded-full bg-[var(--ct-accent)] text-white text-[10px] cursor-pointer shadow-lg border-0"
                >
                  {newCount} new
                </button>
              </div>
            )}
            {filtered.length === 0 && (
              <div className="text-xs text-[var(--ct-muted-fg)] text-center py-8">
                No activity yet
              </div>
            )}
            {filtered.map((ev, i) => {
              const day = dayLabel(ev.ts_ms);
              const showDay = day !== lastDay;
              lastDay = day;
              const opColor = OP_COLORS[ev.op] || "#888";
              const srcColor = colorForSource(ev.source);
              return (
                <div key={`${ev.ts_ms}-${i}`}>
                  {showDay && (
                    <div className="flex items-center gap-2 px-3 py-1.5">
                      <span className="text-[9px] font-semibold uppercase tracking-[0.25em] text-[var(--ct-muted-fg)]">
                        {day}
                      </span>
                      <span className="flex-1 h-px bg-[var(--ct-border)]" />
                    </div>
                  )}
                  <div className="flex items-start gap-2.5 px-3 py-1.5 border-b border-white/[0.04] hover:bg-[var(--ct-hover-bg)] transition-colors duration-75">
                    <span className="w-[56px] shrink-0 text-[10px] font-mono tabular-nums text-[var(--ct-muted-fg)]">
                      {formatTime(ev.ts_ms)}
                    </span>
                    <span
                      className="px-1.5 py-px rounded-[3px] text-[9px] font-semibold uppercase tracking-[0.3px] shrink-0 w-[52px] text-center"
                      style={{ background: `${opColor}22`, color: opColor }}
                    >
                      {ev.op}
                    </span>
                    <div className="flex-1 min-w-0">
                      {ev.mem_ids.slice(0, 3).map((id) => {
                        const node = nodes.get(id);
                        const kind = node?.kind;
                        const kColor = kind ? colorForKind(kind) : undefined;
                        return (
                          <div key={id} className="flex items-center gap-1.5">
                            {kColor && (
                              <span
                                className="px-1 py-px rounded-[2px] text-[9px]"
                                style={{ background: `${kColor}22`, color: kColor }}
                              >
                                {kind}
                              </span>
                            )}
                            <span
                              className="text-[11px] text-[var(--ct-fg)] truncate cursor-pointer hover:underline hover:decoration-[var(--ct-accent)]"
                              onClick={() => onNavigateToItem(id)}
                            >
                              {node?.title || id.slice(0, 20)}
                            </span>
                          </div>
                        );
                      })}
                      {ev.mem_ids.length > 3 && (
                        <span className="text-[10px] text-[var(--ct-muted-fg)]">
                          +{ev.mem_ids.length - 3} more
                        </span>
                      )}
                      {ev.mem_ids.length === 0 && ev.op === "search" && (
                        <span className="text-[11px] text-[var(--ct-fg)] italic">search query</span>
                      )}
                      <div className="text-[9px] text-[var(--ct-muted-fg)] mt-0.5 flex items-center gap-1.5">
                        <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: srcColor }} />
                        <span>
                          src: {ev.source.worker_id || ev.source.pane_id?.slice(0, 8) || ev.source.kind}
                        </span>
                        {ev.source.agent_provider && <span>({ev.source.agent_provider})</span>}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {/* Workers column */}
        <div className="flex flex-col min-h-0">
          <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--ct-border)]">
            <Cpu size={12} className="text-[var(--ct-muted-fg)]" />
            <span className="text-[10px] uppercase tracking-[0.2em] text-[var(--ct-muted-fg)]">Workers</span>
            {live.length > 0 && (
              <span className="text-[9px] px-1.5 py-px rounded-full bg-[#34d399]/20 text-[#34d399] tracking-wider">
                {live.length} live
              </span>
            )}
            <button
              onClick={() => setSpawnOpen(true)}
              className="ml-auto flex items-center gap-1 px-2 py-0.5 rounded text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] border border-[var(--ct-border)] hover:border-[var(--ct-fg)] bg-transparent cursor-pointer"
              title="Spawn worker"
            >
              <PlusCircle size={11} />
              spawn
            </button>
            <button
              onClick={() => refreshWorkers()}
              className="px-1.5 py-0.5 rounded text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] bg-transparent border-0 cursor-pointer"
              title="Refresh"
            >
              <RefreshCw size={11} />
            </button>
          </div>

          <div className="flex-1 overflow-y-auto px-3 py-2 space-y-3">
            {workersLoading && (
              <div className="text-[11px] text-[var(--ct-muted-fg)]">loading…</div>
            )}
            {!workersLoading && live.length === 0 && recent.length === 0 && (
              <div className="text-[11px] text-[var(--ct-muted-fg)] text-center py-6">
                No workers running. Spawn one or wait for the next dreaming session.
              </div>
            )}
            {live.length > 0 && (
              <div>
                <div className="text-[9px] uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-1.5">
                  In flight
                </div>
                <div className="space-y-1">
                  {live.map((w) => (
                    <WorkerCard
                      key={w.worker_id}
                      worker={w}
                      onOpen={() => setLogWorker(w)}
                      onKill={() => handleKill(w.worker_id)}
                    />
                  ))}
                </div>
              </div>
            )}
            {recent.length > 0 && (
              <div>
                <div className="text-[9px] uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-1.5">
                  Recent
                </div>
                <div className="space-y-1">
                  {recent.map((w) => (
                    <WorkerCard
                      key={w.worker_id}
                      worker={w}
                      onOpen={() => setLogWorker(w)}
                      onKill={null}
                    />
                  ))}
                </div>
              </div>
            )}

            {/* Librarian status */}
            <div className="pt-1">
              <div className="text-[9px] uppercase tracking-[0.25em] text-[var(--ct-muted-fg)] mb-1.5">
                Librarian
              </div>
              <div className="flex items-center gap-3 px-3 py-2 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-hover-bg)]/40">
                <div
                  className="w-8 h-8 rounded-md flex items-center justify-center shrink-0"
                  style={{
                    background: memCfg?.file_sync_enabled
                      ? "rgba(96,165,250,0.15)"
                      : "rgba(156,163,175,0.10)",
                    color: memCfg?.file_sync_enabled ? "#60a5fa" : "#9ca3af",
                  }}
                >
                  <Library size={14} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-[11px] text-[var(--ct-fg)]">File sync</div>
                  <div className="text-[10px] text-[var(--ct-muted-fg)] flex items-center gap-1.5">
                    <Files size={10} />
                    <span className="tabular-nums">
                      {syncStatus?.tracked_files ?? 0} files
                    </span>
                    {syncStatus && syncStatus.stale_files > 0 && (
                      <span className="text-[#fbbf24]">
                        · {syncStatus.stale_files} stale
                      </span>
                    )}
                    <span className="opacity-50">·</span>
                    <span>every {memCfg?.file_sync_interval_minutes ?? 60}m</span>
                  </div>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={!!memCfg?.file_sync_enabled}
                  onClick={() => toggleSync(!memCfg?.file_sync_enabled)}
                  className={`relative inline-flex h-5 w-9 items-center rounded-full transition-all border-0 cursor-pointer ${
                    memCfg?.file_sync_enabled
                      ? "bg-[#60a5fa] shadow-[0_0_10px_rgba(96,165,250,0.4)]"
                      : "bg-[var(--ct-border)]"
                  }`}
                >
                  <span
                    className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                      memCfg?.file_sync_enabled ? "translate-x-4" : "translate-x-0.5"
                    }`}
                  />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {spawnOpen && (
        <MemorySpawnWorkerDialog
          onSpawn={async (req) => { await spawn(req); }}
          onClose={() => setSpawnOpen(false)}
        />
      )}
      {logWorker && (
        <AgentRunLogPopup worker={logWorker} api={api} onClose={() => setLogWorker(null)} />
      )}
    </div>
  );
}

interface WorkerCardProps {
  worker: GraphWorkerInfo;
  onOpen: () => void;
  onKill: (() => void) | null;
}

function WorkerCard({ worker, onOpen, onKill }: WorkerCardProps) {
  const isDone =
    worker.status === "done" || worker.status === "error" || worker.status === "orphaned";
  const color = STATUS_COLORS[worker.status];
  const elapsed = (() => {
    const end = isDone && worker.done_at ? worker.done_at : Date.now();
    const diff = end - worker.spawned_at;
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    return `${mins}m ${secs % 60}s`;
  })();
  const intent = worker.intent.replace(/^extract:/, "").replace(/^dreaming:/, "REM:");
  return (
    <div
      onClick={onOpen}
      className="flex items-center gap-2 px-2.5 py-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)]/60 hover:border-[var(--ct-accent)] cursor-pointer transition-colors"
      title={isDone ? "View log" : "View live output"}
    >
      <span
        className="w-1.5 h-1.5 rounded-full shrink-0"
        style={{
          background: color,
          boxShadow: !isDone ? `0 0 8px ${color}` : undefined,
        }}
      />
      <span className="flex-1 text-[11px] truncate text-[var(--ct-fg)]">{intent}</span>
      {worker.model && (
        <span className="text-[9px] text-[var(--ct-muted-fg)] shrink-0 truncate max-w-[80px]">
          {worker.model}
        </span>
      )}
      <span className="text-[9px] tabular-nums text-[var(--ct-muted-fg)] shrink-0">{elapsed}</span>
      <span
        className="text-[8px] uppercase tracking-wider px-1.5 py-px rounded-full shrink-0"
        style={{ background: `${color}22`, color }}
      >
        {worker.status}
      </span>
      {onKill && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onKill();
          }}
          className="text-[10px] px-1 text-[var(--ct-muted-fg)] hover:text-[#f87171] bg-transparent border-0 cursor-pointer"
          title="Kill"
        >
          ✕
        </button>
      )}
    </div>
  );
}

export const MemoryActivityTab = memo(MemoryActivityTabImpl);
