import type { HTMLAttributes } from "react";
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
}

const STATUS_ORDER: readonly PlanStatusKind[] = ["todo", "review", "blocked", "paused", "done"];

export function PlanStatusBreakdown({
  counts,
  className = "",
  itemTestIdPrefix,
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
            style={{ color: PLAN_STATUS_COLORS[status] }}
          >
            <span className="ct-plan-status-breakdown-count">{counts[status]}</span>
            <span className="ct-plan-status-breakdown-label"> {status}</span>
          </span>
        </span>
      ))}
    </span>
  );
}
