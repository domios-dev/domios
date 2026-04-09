import { memo, useEffect, useRef } from "react";
import type { MemActivityEvent, MemOp } from "./useActivityPulses";
import { colorForSource } from "./useActivityPulses";

interface Props {
  events: MemActivityEvent[];
  onSelect: (memId: string) => void;
}

const OP_COLORS: Record<MemOp, string> = {
  read: "#60a5fa",
  search: "#38bdf8",
  recall: "#a78bfa",
  write: "#34d399",
  update: "#fbbf24",
  delete: "#f87171",
  dream: "#f472b6",
};

function relTime(tsMs: number, now: number): string {
  const d = Math.max(0, now - tsMs);
  if (d < 1000) return "now";
  if (d < 60_000) return `${Math.floor(d / 1000)}s ago`;
  if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`;
  return `${Math.floor(d / 86_400_000)}d ago`;
}

function MemoryActivityFeedImpl({ events, onSelect }: Props) {
  const listRef = useRef<HTMLDivElement>(null);
  const userScrolledRef = useRef(false);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const onScroll = () => {
      userScrolledRef.current = el.scrollTop > 8;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    if (!userScrolledRef.current) el.scrollTop = 0;
  }, [events]);

  const now = Date.now();

  return (
    <div className="absolute top-0 left-0 bottom-0 w-[280px] flex flex-col z-[5] text-xs bg-[rgba(17,17,17,0.92)] border-r border-[var(--ct-border)] text-[var(--ct-fg)]">
      <div className="px-3 py-2.5 border-b border-[var(--ct-border)] font-semibold text-xs uppercase tracking-[0.3px] text-[var(--ct-muted-fg)]">
        Recent activity
      </div>
      <div ref={listRef} className="flex-1 overflow-y-auto py-1">
        {events.length === 0 && (
          <div className="px-3 py-4 text-[11px] text-[var(--ct-muted-fg)]">
            No activity yet.
          </div>
        )}
        {events.map((ev, i) => {
          const dotColor = colorForSource(ev.source);
          const opColor = OP_COLORS[ev.op] ?? "#888";
          const label =
            ev.mem_ids.length > 1
              ? `${ev.mem_ids.length} items`
              : ev.mem_ids[0] ?? "—";
          return (
            <div
              key={`${ev.ts_ms}-${i}`}
              onClick={() => {
                if (ev.mem_ids[0]) onSelect(ev.mem_ids[0]);
              }}
              className="flex items-center gap-2 px-3 py-1.5 border-b border-white/[0.04] hover:bg-white/[0.04]"
              style={{ cursor: ev.mem_ids[0] ? "pointer" : "default" }}
            >
              <div
                className="w-2 h-2 rounded-full shrink-0"
                style={{ background: dotColor, boxShadow: `0 0 6px ${dotColor}` }}
              />
              <div
                className="px-1.5 py-px rounded-[3px] text-[10px] font-semibold uppercase tracking-[0.3px] shrink-0"
                style={{ background: `${opColor}22`, color: opColor }}
              >
                {ev.op}
              </div>
              <div
                className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-[var(--ct-fg)]"
                title={ev.mem_ids.join(", ")}
              >
                {label}
              </div>
              <div className="text-[10px] text-[var(--ct-muted-fg)] shrink-0">
                {relTime(ev.ts_ms, now)}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export const MemoryActivityFeed = memo(MemoryActivityFeedImpl);
