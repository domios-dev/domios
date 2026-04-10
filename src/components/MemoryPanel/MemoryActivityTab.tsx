import { memo, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig } from "../../lib/types";
import type { MemNode } from "./types";
import { colorForKind } from "./nodeKind";
import type { MemActivityEvent } from "./useActivityPulses";
import { colorForSource } from "./useActivityPulses";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  onNavigateToItem: (id: string) => void;
}

const OP_COLORS: Record<string, string> = {
  write: "#10b981",
  read: "#3b82f6",
  search: "#a855f7",
  update: "#eab308",
  delete: "#ef4444",
  recall: "#14b8a6",
  dream: "#6366f1",
};

const MAX_EVENTS = 500;

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
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
  const scrollRef = useRef<HTMLDivElement>(null);
  const newCountRef = useRef(0);
  const [newCount, setNewCount] = useState(0);

  // Fetch existing events on mount
  useEffect(() => {
    let active = true;
    const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
    fetch(`http://${host}:${api.port}/api/mem/activity/recent?since_ms=0`, {
      headers: api.token ? { Authorization: `Bearer ${api.token}` } : {},
    })
      .then(r => r.json())
      .then(data => {
        if (!active) return;
        const evts = (data.events || []) as MemActivityEvent[];
        evts.sort((a: MemActivityEvent, b: MemActivityEvent) => b.ts_ms - a.ts_ms);
        setEvents(evts.slice(0, MAX_EVENTS));
      })
      .catch(() => {});
    return () => { active = false; };
  }, [api]);

  // Listen for live events
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    if (isTauri) {
      listen<MemActivityEvent>("mem-activity", (ev) => {
        if (!active) return;
        setEvents(prev => {
          const next = [ev.payload, ...prev];
          if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
          return next;
        });
        if (!autoScroll) {
          newCountRef.current++;
          setNewCount(newCountRef.current);
        }
      }).then(fn => {
        if (!active) { fn(); return; }
        unlisten = fn;
        setIsLive(true);
      }).catch(() => {});
    }
    return () => { active = false; if (unlisten) unlisten(); };
  }, [api, autoScroll]);

  const filtered = opFilter
    ? events.filter(e => e.op === opFilter)
    : events;

  const scrollToTop = useCallback(() => {
    scrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
    setAutoScroll(true);
    newCountRef.current = 0;
    setNewCount(0);
  }, []);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (el.scrollTop > 50 && autoScroll) setAutoScroll(false);
    if (el.scrollTop < 10 && !autoScroll) {
      setAutoScroll(true);
      newCountRef.current = 0;
      setNewCount(0);
    }
  }, [autoScroll]);

  // Group by day
  let lastDay = "";

  return (
    <div className="absolute inset-0 flex flex-col">
      {/* Filters */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--ct-border)]">
        <select
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs text-[var(--ct-fg)] cursor-pointer"
          value={opFilter}
          onChange={e => setOpFilter(e.target.value)}
        >
          <option value="">All ops</option>
          {Object.keys(OP_COLORS).map(op => <option key={op} value={op}>{op}</option>)}
        </select>
        {opFilter && (
          <button onClick={() => setOpFilter("")} className="text-[10px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] cursor-pointer bg-transparent border-0">Clear</button>
        )}
        <span className="ml-auto text-[10px] text-[var(--ct-muted-fg)] tabular-nums">
          {filtered.length} events
        </span>
      </div>

      {/* Event log */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto" onScroll={handleScroll}>
        {/* New events pill */}
        {newCount > 0 && !autoScroll && (
          <div className="sticky top-0 z-10 flex justify-center py-1">
            <button
              onClick={scrollToTop}
              className="px-3 py-1 rounded-full bg-[var(--ct-accent)] text-white text-[10px] cursor-pointer shadow-lg"
            >
              {newCount} new events
            </button>
          </div>
        )}

        {filtered.length === 0 && (
          <div className="text-xs text-[var(--ct-muted-fg)] text-center py-8">No activity yet</div>
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
                  <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--ct-muted-fg)]">{day}</span>
                  <span className="flex-1 h-px bg-[var(--ct-border)]" />
                </div>
              )}
              <div className="flex items-start gap-2.5 px-3 py-1.5 border-b border-white/[0.04] hover:bg-[var(--ct-hover-bg)] transition-colors duration-75">
                <span className="w-[56px] shrink-0 text-[11px] font-mono tabular-nums text-[var(--ct-muted-fg)]">
                  {formatTime(ev.ts_ms)}
                </span>
                <span
                  className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase tracking-[0.3px] shrink-0 w-[52px] text-center"
                  style={{ background: `${opColor}22`, color: opColor }}
                >
                  {ev.op}
                </span>
                <div className="flex-1 min-w-0">
                  {ev.mem_ids.slice(0, 3).map(id => {
                    const node = nodes.get(id);
                    const kind = node?.kind;
                    const kColor = kind ? colorForKind(kind) : undefined;
                    return (
                      <div key={id} className="flex items-center gap-1.5">
                        {kColor && (
                          <span className="px-1 py-px rounded-[2px] text-[9px]" style={{ background: `${kColor}22`, color: kColor }}>{kind}</span>
                        )}
                        <span
                          className="text-xs text-[var(--ct-fg)] truncate cursor-pointer hover:underline hover:decoration-[var(--ct-accent)]"
                          onClick={() => onNavigateToItem(id)}
                        >
                          {node?.title || id.slice(0, 20)}
                        </span>
                      </div>
                    );
                  })}
                  {ev.mem_ids.length > 3 && (
                    <span className="text-[10px] text-[var(--ct-muted-fg)]">+{ev.mem_ids.length - 3} more</span>
                  )}
                  {ev.mem_ids.length === 0 && ev.op === "search" && (
                    <span className="text-xs text-[var(--ct-fg)] italic">search query</span>
                  )}
                  <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5 flex items-center gap-1.5">
                    <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: srcColor }} />
                    <span>src: {ev.source.worker_id || ev.source.pane_id?.slice(0, 8) || ev.source.kind}</span>
                    {ev.source.agent_provider && <span>({ev.source.agent_provider})</span>}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Status bar */}
      <div className="flex items-center gap-2 px-3 py-1 border-t border-[var(--ct-border)] text-[10px] text-[var(--ct-muted-fg)]">
        <span className="flex items-center gap-1">
          <span className={`w-1.5 h-1.5 rounded-full ${isLive ? "bg-[#10b981] animate-pulse" : "bg-[#eab308]"}`} />
          {isLive ? "Live" : "Polling"}
        </span>
        <label className="ml-auto flex items-center gap-1 cursor-pointer hover:text-[var(--ct-fg)]">
          <input type="checkbox" checked={autoScroll} onChange={e => setAutoScroll(e.target.checked)} className="w-3 h-3" />
          auto-scroll
        </label>
      </div>
    </div>
  );
}

export const MemoryActivityTab = memo(MemoryActivityTabImpl);
