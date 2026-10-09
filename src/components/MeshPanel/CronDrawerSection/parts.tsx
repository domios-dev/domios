import { useId, type CSSProperties, type ReactNode } from "react";
import { AUTOMATION_LABEL, AUTOMATION_PRIMARY, AUTOMATION_TONE, AUTOMATION_TOOL } from "../../Automation/styles";
import type { InventoryOperation } from "../types";

export const CRON_LABEL = AUTOMATION_LABEL;
export const CRON_PRIMARY = AUTOMATION_PRIMARY;
export const CRON_PILL = AUTOMATION_TOOL;
export const CRON_DANGER = `${AUTOMATION_TOOL} ct-bar-pill--tinted`;
export const CRON_DANGER_TONE = { "--ct-bar-pill-tone": "var(--ct-red)" } as CSSProperties;

export function CronCard({ label, icon, aside, children, bodyless = false }: { label: string; icon: ReactNode; aside?: ReactNode; children: ReactNode; bodyless?: boolean }) {
  const headingId = useId();
  return <section className="ct-card ct-card--static ct-job-section" aria-labelledby={headingId} style={{ "--ct-card-tone": AUTOMATION_TONE.cron } as CSSProperties}>
    <div className="ct-card__head">
      <span className="ct-card__icon" aria-hidden="true">{icon}</span>
      <h3 id={headingId} className="ct-detail__heading">{label}</h3>
      {aside}
    </div>
    {bodyless ? children : <div className="ct-card__body ct-job-section__body">{children}</div>}
  </section>;
}

export function Unavailable({ operation, reason }: { operation: InventoryOperation; reason?: string }) {
  return <p className="text-xs text-[var(--ct-yellow)]" data-testid={`mesh-operation-unavailable-${operation}`}>Unavailable{reason ? `: ${reason}` : ""}</p>;
}

export function runTone(status: string): string {
  if (status === "Running") return "var(--ct-accent)";
  if (status === "AwaitingOwnerPrompt") return "var(--ct-yellow)";
  if (status === "Success") return "var(--ct-green)";
  if (status === "Cancelled") return "var(--ct-muted)";
  return "var(--ct-red)";
}

export function runStatusLabel(status: string): string {
  if (status.toLowerCase() === "awaitingownerprompt") return "Awaiting owner";
  return status.charAt(0).toUpperCase() + status.slice(1).toLowerCase();
}

export function formatRunDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  if (total < 60) return `${total}s`;
  if (total < 3600) return `${Math.floor(total / 60)}m ${total % 60}s`;
  return `${Math.floor(total / 3600)}h ${Math.floor((total % 3600) / 60)}m`;
}
