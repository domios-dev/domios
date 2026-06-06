import type { Service } from "./types";

const STATUS_COLOR: Record<string, string> = {
  healthy: "var(--ct-green)",
  degraded: "var(--ct-yellow)",
  down: "var(--ct-red)",
  unknown: "var(--ct-muted)",
};

const GRID_COLS = "14px 2fr 1fr 80px 70px 24px";

interface Props {
  service: Service;
  host: string;
  onClick: () => void;
}

export function ServiceRow({ service, host, onClick }: Props) {
  const color = STATUS_COLOR[service.health.status] ?? "var(--ct-muted)";
  const uptime = service.health.uptime_secs
    ? `${Math.floor(service.health.uptime_secs / 3600)}h`
    : "-";
  const staleTitle =
    service.peer_online === false
      ? "Last known state — peer is offline"
      : "Last known state — peer unreachable";
  return (
    <div
      data-testid="service-row"
      data-service-name={service.name}
      data-source-peer={service.source_peer ?? "local"}
      data-stale={String(!!service.stale)}
      onClick={onClick}
      className={`grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] cursor-pointer hover:bg-[var(--ct-titlebar-bg)]${service.stale ? " opacity-60" : ""}`}
      style={{ gridTemplateColumns: GRID_COLS }}
    >
      <span style={{ color }}>{service.stale ? "◌" : "●"}</span>
      <span className="text-[var(--ct-fg)] text-xs truncate" title={service.name}>
        {service.name}
        {service.stale && (
          <span
            data-testid="service-stale-badge"
            className="ml-1.5 text-[9px] uppercase text-[var(--ct-yellow)]"
            title={staleTitle}
          >
            cached
          </span>
        )}
      </span>
      <span className="text-[var(--ct-muted)] text-[11px] truncate" title={host}>
        {host}
      </span>
      <span className="text-[10px] uppercase" style={{ color }}>
        {service.health.status}
      </span>
      <span className="text-[var(--ct-muted)] text-[10px]">{uptime}</span>
      <span className="text-[var(--ct-muted)]">›</span>
    </div>
  );
}

export const SERVICE_GRID_COLS = GRID_COLS;
