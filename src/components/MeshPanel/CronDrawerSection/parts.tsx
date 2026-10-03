import type { ReactNode } from "react";
import { Card } from "../../shared/Card";
import { UI_BUTTON } from "../../shared/design";
import type { InventoryOperation } from "../types";

export const CRON_LABEL = "text-[0.6875rem] font-medium uppercase tracking-wider text-[var(--ct-muted)]";
export const CRON_TEXT = "text-[length:var(--ct-font-size-sm)] leading-relaxed";
export const CRON_PRIMARY = `${UI_BUTTON.primary} ${UI_BUTTON.compact} gap-1.5 disabled:cursor-not-allowed disabled:opacity-50`;
export const CRON_SECONDARY = `${UI_BUTTON.base} ${UI_BUTTON.compact} gap-1.5 border border-[var(--ct-border)] text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] disabled:cursor-not-allowed disabled:opacity-50`;
export const CRON_DANGER = `${UI_BUTTON.danger} ${UI_BUTTON.compact} gap-1.5 border border-[color-mix(in_srgb,var(--ct-red)_35%,transparent)] disabled:cursor-not-allowed disabled:opacity-50`;
export const CRON_QUIET = `${UI_BUTTON.ghost} ${UI_BUTTON.compact} gap-1.5 disabled:cursor-not-allowed disabled:opacity-50`;

export function CronCard({ label, aside, children }: { label: string; aside?: ReactNode; children: ReactNode }) {
  return <Card className="flex min-w-0 flex-col gap-3 p-4 sm:p-5">
    <div className="flex items-center justify-between gap-3"><h3 className={CRON_LABEL}>{label}</h3>{aside}</div>
    {children}
  </Card>;
}

export function Unavailable({ operation, reason }: { operation: InventoryOperation; reason?: string }) {
  return <p className="text-xs text-[var(--ct-yellow)]" data-testid={`mesh-operation-unavailable-${operation}`}>Unavailable{reason ? `: ${reason}` : ""}</p>;
}

export function runIcon(status: string): string {
  if (status === "Running") return "●";
  if (status === "AwaitingOwnerPrompt") return "!";
  if (status === "Success") return "✓";
  if (status === "Cancelled") return "■";
  return "✗";
}

export function runColor(status: string): string {
  if (status === "Running") return "text-[var(--ct-accent)] animate-pulse";
  if (status === "AwaitingOwnerPrompt") return "text-[var(--ct-yellow)]";
  if (status === "Success") return "text-[var(--ct-green)]";
  if (status === "Cancelled") return "text-[var(--ct-muted-fg)]";
  return "text-[var(--ct-red)]";
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
