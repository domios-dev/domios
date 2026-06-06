import { useMemo, useState } from "react";
import { ServiceRow, SERVICE_GRID_COLS } from "./ServiceRow";
import type { Service } from "./types";

interface Props {
  services: Service[];
  thisMachineName: string;
  onServiceClick: (service: Service) => void;
}

const SELECT_CLASS =
  "text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[var(--ct-fg)]";

const STATUS_RANK: Record<string, number> = {
  down: 0,
  degraded: 1,
  unknown: 2,
  healthy: 3,
};

export function ServicesTable({
  services,
  thisMachineName,
  onServiceClick,
}: Props) {
  const [hostFilter, setHostFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<string>("all");

  const daemons = useMemo(() => services.filter((s) => s.kind === "daemon"), [services]);
  const hosts = useMemo(() => {
    const set = new Set<string>([thisMachineName]);
    for (const s of daemons) set.add(s.source_peer ?? thisMachineName);
    return Array.from(set);
  }, [daemons, thisMachineName]);

  const filtered = useMemo(() => {
    const out = daemons.filter((s) => {
      const host = s.source_peer ?? thisMachineName;
      if (hostFilter !== "all" && host !== hostFilter) return false;
      if (statusFilter !== "all" && s.health.status !== statusFilter)
        return false;
      return true;
    });
    out.sort((a, b) => {
      const ha = a.source_peer ?? thisMachineName;
      const hb = b.source_peer ?? thisMachineName;
      if (ha !== hb) return ha.localeCompare(hb);
      const ra = STATUS_RANK[a.health.status] ?? 99;
      const rb = STATUS_RANK[b.health.status] ?? 99;
      if (ra !== rb) return ra - rb;
      return a.name.localeCompare(b.name);
    });
    return out;
  }, [daemons, hostFilter, statusFilter, thisMachineName]);

  return (
    <div className="bg-[var(--ct-bg)] border-t border-[var(--ct-border)] flex-1 min-w-0">
      <div className="flex items-center gap-3 px-4 py-2 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] text-[10px] uppercase text-[var(--ct-muted)]">
        <span className="flex-1">
          Services ({filtered.length})
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
        <select
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
          className={SELECT_CLASS}
        >
          <option value="all">all statuses</option>
          <option value="healthy">healthy</option>
          <option value="degraded">degraded</option>
          <option value="down">down</option>
          <option value="unknown">unknown</option>
        </select>
      </div>
      <div
        className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] text-[9px] uppercase text-[var(--ct-muted)] sticky top-0 bg-[var(--ct-bg)]"
        style={{ gridTemplateColumns: SERVICE_GRID_COLS }}
      >
        <span />
        <span>Name</span>
        <span>Host</span>
        <span>Status</span>
        <span>Uptime</span>
        <span />
      </div>
      {filtered.map((s) => (
        <ServiceRow
          key={`${s.source_peer ?? "local"}:${s.id}`}
          service={s}
          host={s.source_peer ?? thisMachineName}
          onClick={() => onServiceClick(s)}
        />
      ))}
      {filtered.length === 0 && (
        <div className="px-4 py-3 text-center text-[var(--ct-muted)] text-xs">
          No services match filter.
        </div>
      )}
    </div>
  );
}
