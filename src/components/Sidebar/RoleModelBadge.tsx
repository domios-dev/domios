import { RoleBadge } from "./RoleBadge";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

interface RoleModelBadgeProps {
  /** Uppercased leadership label from [`roleBadgeLabel`] — ORCHESTRATOR / PLANNER. */
  role: string;
  /** Formatted model chip (e.g. OPUS). Dropped — never truncated — when the
   *  combined form can't fit; the role always survives. */
  model?: string | null;
  uiFontScale?: number;
}

/** Whether the model chip drops at this scale. Deterministic, keyed to the
 *  font scale — at the SMALLEST scale the combined `ROLE · MODEL` form can't fit
 *  the narrow sidebar, so the model drops there and ONLY there. Never truncates:
 *  the role is identity, the model is a detail that degrades to the tooltip. */
export function shouldDropModelAtScale(uiFontScale?: number): boolean {
  return pickUiFontScaleValue(uiFontScale, [true, false, false, false]);
}

/** Leadership identity in the model-chip slot: `ORCHESTRATOR · OPUS`. Renders
 *  whole (no ellipsis, no clipping) at every scale — the fit-whole contract. */
export function RoleModelBadge({ role, model, uiFontScale }: RoleModelBadgeProps) {
  const trimmedModel = model?.trim() || null;
  const showModel = Boolean(trimmedModel) && !shouldDropModelAtScale(uiFontScale);
  const title = trimmedModel ? `${role} · ${trimmedModel}` : role;
  return (
    <span
      data-testid="leadership-badge"
      data-leadership-role={role}
      className="sidebar-leadership-badge inline-flex items-center gap-1"
      title={title}
    >
      <RoleBadge label={role} title={title} />
      {showModel && (
        <span
          data-testid="leadership-model"
          className="sidebar-leadership-model text-[8px] leading-tight font-mono uppercase tracking-wider text-[var(--ct-muted-fg)]"
        >
          <span aria-hidden className="mr-1 opacity-50">·</span>
          {trimmedModel}
        </span>
      )}
    </span>
  );
}
