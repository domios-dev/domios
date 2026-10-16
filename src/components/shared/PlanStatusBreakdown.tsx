import type { CSSProperties, HTMLAttributes } from "react";
import { PLAN_STATUS_COLORS, type PlanStatusKind } from "./SegmentedDonut";

export interface PlanStatusCounts {
  todo: number;
  review: number;
  blocked: number;
  paused: number;
  done: number;
}

export interface PlanStatusBreakdownProps extends HTMLAttributes<HTMLSpanElement> {
  counts: PlanStatusCounts;
  itemTestIdPrefix?: string;
  tone?: "status" | "muted";
}

const STATUS_ORDER: readonly PlanStatusKind[] = ["todo", "review", "blocked", "done"];

export function PlanStatusBreakdown({
  counts,
  className = "",
  itemTestIdPrefix,
  tone = "status",
  "aria-label": ariaLabel,
  ...props
}: PlanStatusBreakdownProps) {
  const summary = STATUS_ORDER.map((status) => `${counts[status]} ${status}`).join(", ");

  return (
    <span
      className={`ct-plan-status-breakdown ${className}`.trim()}
      aria-label={ariaLabel ?? summary}
      {...props}
    >
      {STATUS_ORDER.map((status, index) => (
        <span key={status} className="ct-plan-status-breakdown-segment">
          {index > 0 && (
            <span
              data-testid="plan-status-separator"
              className="ct-plan-status-breakdown-separator"
              aria-hidden
            >
              {" · "}
            </span>
          )}
          <span
            data-testid={itemTestIdPrefix ? `${itemTestIdPrefix}-${status}` : undefined}
            data-status={status}
            className="ct-plan-status-breakdown-item"
            style={{
              "--ct-plan-status-color": PLAN_STATUS_COLORS[status],
              color: tone === "muted" ? "var(--ct-plan-status-foreground, var(--ct-muted-fg))" : PLAN_STATUS_COLORS[status],
            } as CSSProperties}
          >
            <span className="ct-plan-status-breakdown-count">{counts[status]}</span>
            <span className="ct-plan-status-breakdown-label"> {status}</span>
          </span>
        </span>
      ))}
    </span>
  );
}
