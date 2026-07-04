import { roleBadgeTestId } from "./orchestrationSections";

interface RoleBadgeProps {
  /** Uppercased role label from [`roleBadgeLabel`] — e.g. ORCHESTRATOR, PLANNER. */
  label: string;
  /** Model, moved off the badge face into the tooltip so it stays discoverable. */
  title?: string;
  className?: string;
}

/** Occupies the model-chip slot for group leadership (Orchestrator/Planner),
 *  making the subordination model legible at a glance. Accent-tinted so it
 *  reads as an identity, not the muted model chip it replaces. */
export function RoleBadge({ label, title, className = "" }: RoleBadgeProps) {
  return (
    <span
      data-testid={roleBadgeTestId(label)}
      data-role-badge={label}
      className={`sidebar-tab-badge sidebar-role-badge px-1 py-0 text-[8px] leading-tight rounded font-mono uppercase tracking-wider border border-[var(--ct-accent)]/40 bg-[var(--ct-accent)]/10 text-[var(--ct-accent)] ${className}`}
      title={title}
    >
      {label}
    </span>
  );
}
