import type { Service } from "./types";

const GRID_COLS = "14px 2fr 1fr 90px 70px 24px";

interface Props {
  service: Service;
  host: string;
  onClick: () => void;
}

function fmtNextRun(ts?: number | null): string {
  if (!ts) return "-";
  const d = new Date(ts);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay
    ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function statusInfo(svc: Service): { label: string; color: string } {
  if (!svc.enabled) return { label: "disabled", color: "var(--ct-muted)" };
  if (svc.last_run_status === "running") return { label: "running", color: "var(--ct-yellow)" };
  if (svc.last_run_status === "failed") return { label: "failed", color: "var(--ct-red)" };
  if (svc.next_run) return { label: `next ${fmtNextRun(svc.next_run)}`, color: "var(--ct-fg)" };
  return { label: "idle", color: "var(--ct-muted)" };
}

export function CronRow({ service, host, onClick }: Props) {
  const { label, color } = statusInfo(service);
  return (
    <div
      onClick={onClick}
      className="grid items-center px-4 py-1.5 border-b border-[var(--ct-border)] cursor-pointer hover:bg-[var(--ct-titlebar-bg)]"
      style={{ gridTemplateColumns: GRID_COLS }}
    >
      <span style={{ color }}>●</span>
      <span className="text-[var(--ct-fg)] text-xs truncate" title={service.name}>
        {service.name}
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
