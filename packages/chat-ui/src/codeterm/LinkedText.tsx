import React from "react";
import { linkifyPlanRefs } from "./planRefs";

export type OpenPlan = (planId: string, taskId?: string) => void;

interface Props {
  text: string;
  onOpenPlan?: OpenPlan;
  className?: string;
}

/** Render plain text with `task_<hex>` mentions as clickable plan deep-links. */
export function LinkedText({ text, onOpenPlan, className }: Props) {
  const segments = linkifyPlanRefs(text);
  return (
    <span className={className} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
      {segments.map((seg, i) => {
        if (seg.kind === "text") return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        if (!onOpenPlan) return <React.Fragment key={i}>{seg.value}</React.Fragment>;
        return (
          <button
            key={i}
            type="button"
            data-testid="chat-task-link"
            data-task-id={seg.taskId}
            data-plan-id={seg.planId}
            onClick={(e) => {
              e.stopPropagation();
              onOpenPlan(seg.planId, seg.taskId);
            }}
            className="underline font-mono"
            style={{ color: "var(--ct-accent, #6ea8fe)" }}
          >
            {seg.value}
          </button>
        );
      })}
    </span>
  );
}
