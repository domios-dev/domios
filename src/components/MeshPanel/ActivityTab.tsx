import { useActivityLog } from "./useActivityLog";
import type { ActivitySource, ActivityLevel } from "./types";

const SOURCE_ICON: Record<ActivitySource, string> = { push: "⤳", health: "●", peer: "↔", cron: "⏱" };
const LEVEL_COLOR: Record<ActivityLevel, string> = { info: "var(--ct-fg)", warn: "var(--ct-yellow)", error: "var(--ct-red)" };

interface Props { active: boolean; }
export function ActivityTab({ active }: Props) {
  const events = useActivityLog(active);
  if (events.length === 0) return <div className="p-4 text-[var(--ct-muted-fg)] text-xs">No activity yet.</div>;
  return (
    <div className="overflow-auto h-full">
      {events.map((ev, i) => (
        <div key={ev.id ?? i} className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)]"
          style={{ gridTemplateColumns: "70px 100px 18px 1fr", color: LEVEL_COLOR[ev.level] ?? "var(--ct-fg)" }}>
          <span className="text-[10px] text-[var(--ct-muted-fg)] font-mono">{new Date(ev.occurred_at).toLocaleTimeString()}</span>
          <span className="text-[10px] text-[var(--ct-muted-fg)]">{ev.source_peer ?? "this"}</span>
          <span>{SOURCE_ICON[ev.source] ?? "•"}</span>
          <span className="text-xs">{ev.message}</span>
        </div>
      ))}
    </div>
  );
}
