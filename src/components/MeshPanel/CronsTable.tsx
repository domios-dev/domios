import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useMemo, useState } from "react";
import { CronRow, CRON_GRID_COLS } from "./CronRow";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";

interface Props {
  services: Service[];
  peers: PeerStatus[];
  thisMachineName: string;
  onServiceClick: (service: Service) => void;
}

const SELECT_CLASS =
  "text-[10px] bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-1.5 py-0.5 text-[var(--ct-fg)]";

export function CronsTable({
  services,
  peers,
  thisMachineName,
  onServiceClick,
}: Props) {
  const [hostFilter, setHostFilter] = useState<string>("all");
  const crons = useMemo(() => services.filter((s) => s.kind === "cron"), [services]);
  const peerIssues = useMemo(
    () =>
      peers.filter(
        (p) =>
          !p.pending &&
          (p.services_error || p.services_stale) &&
          (hostFilter === "all" || p.name === hostFilter),
      ),
    [peers, hostFilter],
  );
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
    <div className="bg-[var(--ct-bg)] flex-1 min-w-0">
      <div className="ct-jobs-header flex items-center gap-3 px-4 py-2 text-[10px] uppercase text-[var(--ct-muted)]">
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
        className="grid items-center px-4 pt-3 pb-1.5 text-[9px] uppercase text-[var(--ct-muted)] sticky top-0 bg-[var(--ct-bg)]"
        style={{ gridTemplateColumns: CRON_GRID_COLS }}
      >
        <span />
        <span>Name</span>
        <span>Host</span>
        <span>State</span>
        <span>Schedule</span>
        <span />
      </div>
      <div className="ct-rows">
        {filtered.map((s) => (
          <CronRow
            key={`${s.source_peer_id ?? s.source_peer ?? "local"}:${s.id}`}
            service={s}
            host={s.source_peer ?? thisMachineName}
            onClick={() => onServiceClick(s)}
          />
        ))}
        {peerIssues.map((p) => (
          <div
            key={`cron-issue-${p.instance_id}`}
            data-testid="cron-peer-unavailable"
            className="px-2 py-2 text-[11px] text-[var(--ct-yellow)]"
          >
            {p.name}: cron services {p.services_error ? <InlineErrorNotice error={p.services_error} operation={`reading services on ${p.name}`} /> : "cached / stale"}
          </div>
        ))}
      </div>
      {filtered.length === 0 && peerIssues.length === 0 && (
        <div className="px-4 py-3 text-center text-[var(--ct-muted)] text-xs">
          No crons.
        </div>
      )}
    </div>
  );
}
