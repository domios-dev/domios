import type { HTMLAttributes } from "react";
import { PLAN_STATUS_COLORS, type PlanStatusKind } from "./SegmentedDonut";
import "./plan-status-breakdown.css";

export interface PlanStatusCounts {
  todo: number;
  review: number;
  blocked: number;
  done: number;
}

export interface PlanStatusBreakdownProps extends HTMLAttributes<HTMLSpanElement> {
  counts: PlanStatusCounts;
}

const STATUS_ORDER: readonly PlanStatusKind[] = ["todo", "review", "blocked", "done"];

export function PlanStatusBreakdown({ counts, className = "", ...props }: PlanStatusBreakdownProps) {
  return <span className={`ds-root ds-plan-status-breakdown ${className}`} {...props}>
    {STATUS_ORDER.map((status, index) => <span key={status} className="ds-plan-status-breakdown__segment">
      {index > 0 && <span className="ds-plan-status-breakdown__separator" aria-hidden> · </span>}
      <span className="ds-plan-status-breakdown__item" style={{ color: PLAN_STATUS_COLORS[status] }}><span className="ds-plan-status-breakdown__count">{counts[status]}</span><span className="ds-plan-status-breakdown__label"> {status}</span></span>
    </span>)}
  </span>;
}
