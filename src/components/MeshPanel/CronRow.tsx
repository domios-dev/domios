import type { Service } from "./types";
import { formatFuture } from "../../lib/formatters";

const GRID_COLS = "14px 2fr 1fr 110px 70px 24px";

interface Props {
  service: Service;
  host: string;
  onClick: () => void;
}

function statusInfo(svc: Service): { label: string; color: string } {
  if (!svc.enabled) return { label: "disabled", color: "var(--ct-muted)" };
  if (svc.last_run_status === "running") return { label: "running", color: "var(--ct-yellow)" };
  if (svc.last_run_status === "failed") return { label: "failed", color: "var(--ct-red)" };
  if (svc.next_run) return { label: formatFuture(svc.next_run), color: "var(--ct-fg)" };
  return { label: "idle", color: "var(--ct-muted)" };
}

export function CronRow({ service, host, onClick }: Props) {
  const { label, color } = statusInfo(service);
  return (
    <div
      data-testid="cron-row"
      data-service-name={service.name}
      data-source-peer={service.source_peer ?? "local"}
      data-source-peer-id={service.source_peer_id ?? "local"}
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
            data-testid="cron-stale-badge"
            className="ml-1.5 text-[9px] uppercase text-[var(--ct-yellow)]"
            title={service.peer_online === false ? "Last known cron — peer is offline" : "Last known cron — peer unreachable"}
          >
            cached
          </span>
        )}
      </span>
      <span className="text-[var(--ct-muted)] text-[11px] truncate" title={host}>
        {host}
      </span>
      <span className="text-[10px]" style={{ color }}>
        {label}
      </span>
      <span className="text-[var(--ct-muted)] text-[10px] font-mono truncate" title={service.schedule ?? ""}>
        {service.schedule ?? "-"}
      </span>
      <span className="text-[var(--ct-muted)]">›</span>
    </div>
  );
}

export const CRON_GRID_COLS = GRID_COLS;
