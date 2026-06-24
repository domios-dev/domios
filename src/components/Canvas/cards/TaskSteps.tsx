import type { CSSProperties } from "react";
import type { Milestone } from "../types";
import { MILESTONE_LANE_COLORS } from "../settings";
import { atLeast, type World } from "../worlds";
import { digestMarkdown } from "./digest";

// Thread & Coil step representation (§16.9, §26.2), INTERIOR to a task cell, driven
// by the task's WORLD band (zoom, not click):
//   W0 constellation — kind-colored dots, freshest pulses (a clean far view).
//   W1 thread        — spine + dots + 1-line step TITLES (readable at mid).
//   W2 cards         — mini step cards: kind strip + title + 2-line excerpt + iter.
//   W3 full          — same cards, unclamped text + links.
export interface TaskStepsProps {
  milestones: Milestone[];
  colors?: Map<string, string>;
  world: World;
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

function freshestId(milestones: Milestone[]): string | null {
  let best: Milestone | null = null;
  for (const m of milestones) if (!best || m.ts > best.ts) best = m;
  return best?.id ?? null;
}

export function TaskSteps({ milestones, colors, world }: TaskStepsProps) {
  if (!milestones.length) return null;

  // W0 MASS — constellation: kind-colored dots, the freshest pulses (eye-magnet).
  if (!atLeast(world, "W1")) {
    const shown = milestones.slice(0, 10);
    const extra = milestones.length - shown.length;
    const fresh = freshestId(milestones);
    return (
      <div data-testid="canvas-task-steps" data-step-mode="constellation" style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
        {shown.map((m) => (
          <span
            key={m.id}
            data-testid="canvas-task-step-dot"
            aria-hidden
            className={m.id === fresh ? "ct-canvas-step-live" : undefined}
            style={dot(colorFor(m.kind, colors), 6)}
          />
        ))}
        {extra > 0 && <span style={{ fontSize: "0.66em", opacity: "var(--ct-text-secondary-opacity)" }}>+{extra}</span>}
      </div>
    );
  }

  // W1 THREAD — vertical spine, dot + kind glyph + 1-line step TITLE (readable mid).
  if (!atLeast(world, "W2")) {
    const fresh = freshestId(milestones);
    return (
      <div data-testid="canvas-task-steps" data-step-mode="thread" style={{ display: "flex", flexDirection: "column", gap: 5, position: "relative" }}>
        <span aria-hidden style={{ position: "absolute", left: 4, top: 5, bottom: 5, width: 1.5, background: "var(--ct-border-subtle)" }} />
        {milestones.map((m) => {
          const color = colorFor(m.kind, colors);
          const title = digestMarkdown(m.text, 52);
          return (
            <div key={m.id} data-testid="canvas-task-step-row" style={{ display: "flex", alignItems: "center", gap: 7, position: "relative", zIndex: 1 }}>
              <span data-testid="canvas-task-step-dot" aria-hidden className={m.id === fresh ? "ct-canvas-step-live" : undefined} style={{ ...dot(color, 9), boxShadow: `0 0 0 2px var(--ct-bg)` }} />
              <span style={{ flex: 1, minWidth: 0, fontSize: "0.78em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", opacity: 0.92 }}>
                {title || m.kind.replace(/_/g, " ")}
              </span>
              {m.iteration != null && (
                <span style={{ fontSize: "0.6em", fontFamily: "var(--ct-font-mono)", fontWeight: 700, opacity: 0.7 }}>#{m.iteration}</span>
              )}
            </div>
          );
        })}
      </div>
    );
  }

  // W2/W3 — step CARDS along a spine, each with the milestone text + iteration.
  const clamp = !atLeast(world, "W3");
  return (
    <div data-testid="canvas-task-steps" data-step-mode={clamp ? "cards" : "full"} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {milestones.map((m) => {
        const color = colorFor(m.kind, colors);
        const text = clamp ? digestMarkdown(m.text, 140) : m.text;
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
                  style={
                    clamp
                      ? { marginTop: 2, fontSize: "0.8em", lineHeight: 1.4, opacity: 0.9, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }
                      : { marginTop: 2, fontSize: "0.8em", lineHeight: 1.45, opacity: 0.92, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }
                  }
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
