import type { CSSProperties } from "react";
import { Archive } from "lucide-react";
import type { Plan } from "../types";
import { planCoverageFraction } from "./coverage";
import type { CanvasCardStyleProps } from "./types";

const frameStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-lg)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "var(--ct-shadow-md)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
};

export interface PlanFrameProps extends CanvasCardStyleProps {
  plan: Plan;
  onArchive?: () => void;
}

export function PlanFrame({ plan, style, lod = "near", onArchive }: PlanFrameProps) {
  const coverage = planCoverageFraction(plan);
  const pct = Math.round(coverage * 100);
  const taskCount = plan.tasks.length;
  const doneCount = plan.tasks.filter((t) => t.status === "done").length;

  return (
    <div
      data-testid="canvas-plan-frame"
      data-plan-id={plan.id}
      style={{ ...frameStyle, ...style }}
    >
      <div
        style={{
          padding: "10px 12px",
          borderBottom: "1px solid var(--ct-border-subtle)",
          display: "flex",
          alignItems: "center",
          gap: 8,
          minHeight: 40,
        }}
      >
        <span
          style={{
            fontWeight: 600,
            fontSize: "var(--ct-font-size-md)",
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {plan.title}
        </span>
        {lod !== "far" && (
          <span
            style={{
              fontSize: "var(--ct-font-size-sm)",
              opacity: "var(--ct-text-secondary-opacity)",
            }}
          >
            {doneCount}/{taskCount} tasks
          </span>
        )}
        {onArchive && (
          <button
            type="button"
            data-testid="canvas-plan-archive"
            title="Archive plan"
            onClick={(e) => {
              e.stopPropagation();
              onArchive();
            }}
            onMouseDown={(e) => e.stopPropagation()}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: 26,
              height: 26,
              borderRadius: "var(--ct-radius-sm)",
              border: "1px solid var(--ct-border-subtle)",
              background: "var(--ct-bg)",
              color: "var(--ct-fg)",
              cursor: "pointer",
              flexShrink: 0,
            }}
          >
            <Archive size={14} />
          </button>
        )}
      </div>
      <div style={{ padding: "8px 12px 10px" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: "var(--ct-font-size-sm)",
            opacity: lod === "far" ? "var(--ct-text-tertiary-opacity)" : 1,
          }}
        >
          <span>Coverage</span>
          <div
            style={{
              flex: 1,
              height: 6,
              borderRadius: 3,
              background: "var(--ct-bg-active)",
              overflow: "hidden",
            }}
          >
            <div
              data-testid="canvas-plan-coverage-bar"
              style={{
                width: `${pct}%`,
                height: "100%",
                background: "var(--ct-border-focus)",
                transition: `width var(--ct-duration-normal) var(--ct-ease-decel)`,
              }}
            />
          </div>
          <span data-testid="canvas-plan-coverage-pct">{pct}%</span>
        </div>
      </div>
    </div>
  );
}
