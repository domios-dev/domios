import { useMemo, useState } from "react";
import { CronRow, CRON_GRID_COLS } from "./CronRow";
import type { Service } from "./types";

interface Props {
  services: Service[];
  thisMachineName: string;
  onServiceClick: (service: Service) => void;
}

const SELECT_CLASS =
  "text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[var(--ct-fg)]";

export function CronsTable({
  services,
  thisMachineName,
  onServiceClick,
}: Props) {
  const [hostFilter, setHostFilter] = useState<string>("all");
  const crons = useMemo(() => services.filter((s) => s.kind === "cron"), [services]);
  const hosts = useMemo(() => {
    const set = new Set<string>([thisMachineName]);
    for (const s of crons) set.add(s.source_peer ?? thisMachineName);
    return Array.from(set);
  }, [crons, thisMachineName]);
  const filtered = useMemo(() => {
    const out = crons.filter((s) => {
      const host = s.source_peer ?? thisMachineName;
      return hostFilter === "all" || host === hostFilter;
    });
    out.sort((a, b) => {
      const ha = a.source_peer ?? thisMachineName;
      const hb = b.source_peer ?? thisMachineName;
      if (ha !== hb) return ha.localeCompare(hb);
      return (a.next_run ?? Infinity) - (b.next_run ?? Infinity);
    });
    return out;
  }, [crons, hostFilter, thisMachineName]);

  return (
    <div className="bg-[var(--ct-bg)] border-t border-[var(--ct-border)] flex-1 min-w-0">
      <div className="flex items-center gap-3 px-4 py-2 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[10px] uppercase text-[var(--ct-muted)]">
        <span className="flex-1">
          Crons ({filtered.length})
        </span>
        <select
          value={hostFilter}
          onChange={(e) => setHostFilter(e.target.value)}
          className={SELECT_CLASS}
        >
          <option value="all">all hosts</option>
          {hosts.map((h) => (
            <option key={h} value={h}>
              {h}
            </option>
          ))}
        </select>
      </div>
      <div
        className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] text-[9px] uppercase text-[var(--ct-muted)] sticky top-0 bg-[var(--ct-bg)]"
        style={{ gridTemplateColumns: CRON_GRID_COLS }}
      >
        <span />
        <span>Name</span>
        <span>Host</span>
        <span>State</span>
        <span>Schedule</span>
        <span />
      </div>
      {filtered.map((s) => (
        <CronRow
          key={s.id}
          service={s}
          host={s.source_peer ?? thisMachineName}
          onClick={() => onServiceClick(s)}
        />
      ))}
      {filtered.length === 0 && (
        <div className="px-4 py-3 text-center text-[var(--ct-muted)] text-xs">
          No crons.
        </div>
      )}
    </div>
  );
}
