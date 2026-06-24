import type { CSSProperties } from "react";
import type { Milestone } from "../types";
import { MILESTONE_LANE_COLORS } from "../settings";
import type { CanvasViewMode } from "./types";
import { digestMarkdown } from "./digest";

// Thread & Coil step representation (§16.9), INTERIOR to a task cell. Mode-driven
// (manual, not zoom): Tile = constellation dots, Card = thread + count, Document =
// step CARDS with the milestone text (the "steps are just dots" bug fix).
export interface TaskStepsProps {
  milestones: Milestone[];
  colors?: Map<string, string>;
  mode: CanvasViewMode;
}

function colorFor(kind: string, colors?: Map<string, string>): string {
  const c = colors?.get(kind);
  if (c) return c;
  let h = 0;
  for (let i = 0; i < kind.length; i++) h = (h * 31 + kind.charCodeAt(i)) >>> 0;
  return MILESTONE_LANE_COLORS[h % MILESTONE_LANE_COLORS.length];
}

const dot = (color: string, size = 8): CSSProperties => ({
  width: size,
  height: size,
  borderRadius: "50%",
  flexShrink: 0,
  background: color,
  boxShadow: `0 0 0 2px color-mix(in srgb, ${color} 22%, transparent)`,
});

export function TaskSteps({ milestones, colors, mode }: TaskStepsProps) {
  if (!milestones.length) return null;

  if (mode === "tile") {
    const shown = milestones.slice(0, 8);
    const extra = milestones.length - shown.length;
    return (
      <div data-testid="canvas-task-steps" data-step-mode="tile" style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
        {shown.map((m) => (
          <span key={m.id} data-testid="canvas-task-step-dot" aria-hidden style={dot(colorFor(m.kind, colors), 6)} />
        ))}
        {extra > 0 && <span style={{ fontSize: "0.66em", opacity: "var(--ct-text-secondary-opacity)" }}>+{extra}</span>}
      </div>
    );
  }

  if (mode === "card") {
    const shown = milestones.slice(0, 10);
    return (
      <div data-testid="canvas-task-steps" data-step-mode="card" style={{ display: "flex", alignItems: "center", gap: 5 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 3 }}>
          {shown.map((m) => (
            <span key={m.id} data-testid="canvas-task-step-dot" aria-hidden style={dot(colorFor(m.kind, colors), 7)} />
          ))}
        </div>
        <span style={{ fontSize: "0.72em", opacity: "var(--ct-text-secondary-opacity)" }}>
          {milestones.length} step{milestones.length === 1 ? "" : "s"}
        </span>
      </div>
    );
  }

  // Document — step CARDS along a spine, each with the milestone text + iteration.
  return (
    <div
      data-testid="canvas-task-steps"
      data-step-mode="document"
      style={{ display: "flex", flexDirection: "column", gap: 8 }}
    >
      {milestones.map((m) => {
        const color = colorFor(m.kind, colors);
        const text = digestMarkdown(m.text, 140);
        return (
          <div
            key={m.id}
            data-testid="canvas-task-step-card"
            data-milestone-id={m.id}
            style={{
              display: "flex",
              gap: 8,
              padding: "6px 8px 6px 0",
              borderRadius: "var(--ct-radius-sm)",
              background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)",
              borderLeft: `3px solid ${color}`,
              overflow: "hidden",
            }}
          >
            <div style={{ minWidth: 0, flex: 1, paddingLeft: 8 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <span style={{ fontSize: "0.72em", fontWeight: 700, textTransform: "capitalize", color }}>{m.kind.replace(/_/g, " ")}</span>
                {m.iteration != null && (
                  <span data-testid="canvas-task-step-iteration" style={{ fontSize: "0.62em", fontFamily: "var(--ct-font-mono)", fontWeight: 700, padding: "0 5px", borderRadius: 999, background: `color-mix(in srgb, ${color} 18%, transparent)`, color }}>
                    #{m.iteration}
                  </span>
                )}
              </div>
              {text && (
                <div
                  data-testid="canvas-task-step-text"
                  style={{ marginTop: 2, fontSize: "0.8em", lineHeight: 1.4, opacity: 0.9, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}
                >
                  {text}
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
