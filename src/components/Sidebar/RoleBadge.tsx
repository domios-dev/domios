import { roleBadgeTestId } from "./orchestrationSections";

interface RoleBadgeProps {
  /** Uppercased role label from [`roleBadgeLabel`] — e.g. ORCHESTRATOR, PLANNER. */
  label: string;
  /** Model, moved off the badge face into the tooltip so it stays discoverable. */
  title?: string;
  className?: string;
}

/** Group-leadership identity in the model-chip slot (Orchestrator/Planner).
 *  Borderless accent-colored small-caps — an identity, not a boxed chip. No
 *  border/background: the color alone carries the role. */
export function RoleBadge({ label, title, className = "" }: RoleBadgeProps) {
  return (
    <span
      data-testid={roleBadgeTestId(label)}
      data-role-badge={label}
      className={`sidebar-role-badge text-[8px] leading-tight font-mono font-semibold uppercase tracking-wider text-[var(--ct-accent)] ${className}`}
      title={title}
    >
      {label}
    </span>
  );
}
