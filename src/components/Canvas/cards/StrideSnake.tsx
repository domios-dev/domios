import { Fragment } from "react";
import type { Milestone } from "../types";
import { MILESTONE_LANE_COLORS } from "../settings";
import { atLeast, type World } from "../worlds";
import { digestMarkdown } from "./digest";
import { MilestoneText } from "./MilestoneText";

// §32.5 Strides = a serpentine snake of kind-colored circular beads (Thread & Coil).
// ONE continuous 2px path threads every stride; it wraps as a boustrophedon snake
// (rows alternate direction, semicircular elbow turns). Freshest bead = head
// (pulses); older strides coil toward the tail. Per task-world band the same snake
// gains detail — a stride is a leaf MICRO-WORLD:
//   W0 s0 — 7px dots, capped, on a compact line (mass preview).
//   W1 s1 — 12px beads + kind ring + 1-line labels alternating above/below.
//   W2 s2 — 18px beads + a callout on the line near each bead.
//   W3 s3 — beads bloom into full stride cards; the path stays as a provenance spine.
// Bloom space is RESERVED in the task footprint (§32.4) so nothing reflows on zoom.

export interface StrideSnakeProps {
  milestones: Milestone[];
  colors?: Map<string, string>;
  world: World;
  width: number; // inner content width in px (card width − padding)
  onCopyLink?: (milestoneId: string) => void;
  onFocusStride?: (milestoneId: string) => void;
}

function colorFor(kind: string, colors?: Map<string, string>): string {
  const c = colors?.get(kind);
  if (c) return c;
  let h = 0;
  for (let i = 0; i < kind.length; i++) h = (h * 31 + kind.charCodeAt(i)) >>> 0;
  return MILESTONE_LANE_COLORS[h % MILESTONE_LANE_COLORS.length];
}

function freshestId(milestones: Milestone[]): string | null {
  let best: Milestone | null = null;
  for (const m of milestones) if (!best || m.ts > best.ts) best = m;
  return best?.id ?? null;
}

interface BeadPos {
  x: number;
  y: number;
  row: number;
  col: number;
}

// Boustrophedon bead grid: even rows L→R, odd rows R→L, so consecutive strides are
// always adjacent and the connecting path never crosses itself.
function layoutBeads(n: number, width: number, pad: number, pitch: number, rowH: number, topPad: number): { beads: BeadPos[]; perRow: number; height: number } {
  const usable = Math.max(pitch, width - pad * 2);
  const perRow = Math.max(1, Math.floor(usable / pitch) + 1);
  const beads: BeadPos[] = [];
  for (let i = 0; i < n; i++) {
    const row = Math.floor(i / perRow);
    const colRaw = i % perRow;
    const col = row % 2 === 0 ? colRaw : perRow - 1 - colRaw;
    beads.push({ x: pad + col * pitch, y: topPad + row * rowH, row, col });
  }
  const rows = Math.ceil(n / perRow);
  return { beads, perRow, height: topPad + Math.max(0, rows - 1) * rowH + topPad };
}

// One continuous path through the bead centers: straight runs along a row, a
// semicircular arc at each edge turn (the "coil"/elbow).
function snakePath(beads: BeadPos[], r: number): string {
  if (beads.length === 0) return "";
  let d = `M ${beads[0].x} ${beads[0].y}`;
  for (let i = 1; i < beads.length; i++) {
    const a = beads[i - 1];
    const b = beads[i];
    if (a.row === b.row) {
      d += ` L ${b.x} ${b.y}`;
    } else {
      // Vertical edge turn — same x; bulge outward with a semicircle.
      const sweep = a.col >= b.col ? 1 : 0; // right edge bulges right, left bulges left
      d += ` A ${r} ${r} 0 0 ${sweep} ${b.x} ${b.y}`;
    }
  }
  return d;
}

export function StrideSnake({ milestones, colors, world, width, onCopyLink, onFocusStride }: StrideSnakeProps) {
  if (!milestones.length) return null;
  const fresh = freshestId(milestones);

  // W3 — bloom: a vertical provenance spine on the left, full stride micro-cards
  // to the right. The path is still drawn so the snake reads as one thread.
  if (atLeast(world, "W3")) {
    return (
      <div data-testid="canvas-task-steps" data-step-mode="bloom" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {milestones.map((m, i) => {
          const color = colorFor(m.kind, colors);
          const isFresh = m.id === fresh;
          return (
            <div key={m.id} data-testid="canvas-task-step-card" data-milestone-id={m.id} style={{ display: "flex", gap: 10, position: "relative" }}>
              <div style={{ position: "relative", width: 18, flexShrink: 0 }}>
                {i < milestones.length - 1 && (
                  <span aria-hidden style={{ position: "absolute", left: 8, top: 16, bottom: -10, width: 2, background: "color-mix(in srgb, var(--ct-fg) 14%, transparent)" }} />
                )}
                <span aria-hidden className={isFresh ? "ct-canvas-step-live" : undefined} style={{ position: "absolute", left: 0, top: 2, width: 16, height: 16, borderRadius: "50%", background: color, boxShadow: `0 0 0 3px color-mix(in srgb, ${color} 22%, var(--ct-bg))` }} />
              </div>
              <div style={{ flex: 1, minWidth: 0, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", borderLeft: `3px solid ${color}`, background: "color-mix(in srgb, var(--ct-fg) 2.5%, transparent)", padding: "7px 9px" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span style={{ fontSize: "0.72em", fontWeight: 700, textTransform: "capitalize", color }}>{m.kind.replace(/_/g, " ")}</span>
                  {m.iteration != null && (
                    <span data-testid="canvas-task-step-iteration" style={{ fontSize: "0.62em", fontFamily: "var(--ct-font-mono)", fontWeight: 700, padding: "0 5px", borderRadius: 999, background: `color-mix(in srgb, ${color} 18%, transparent)`, color }}>#{m.iteration}</span>
                  )}
                  <span style={{ flex: 1 }} />
                  {onFocusStride && (
                    <button type="button" data-testid="canvas-stride-focus" title="Focus stride" onClick={(e) => { e.stopPropagation(); onFocusStride(m.id); }} onMouseDown={(e) => e.stopPropagation()} style={miniBtn()}>◎</button>
                  )}
                  {onCopyLink && (
                    <button type="button" data-testid="canvas-stride-copy" title="Copy stride link" onClick={(e) => { e.stopPropagation(); onCopyLink(m.id); }} onMouseDown={(e) => e.stopPropagation()} style={miniBtn()}>⧉</button>
                  )}
                </div>
                {m.text.trim() && (
                  <div data-canvas-selectable style={{ marginTop: 3, fontSize: "0.82em", lineHeight: 1.5 }}>
                    <MilestoneText text={m.text} testId="canvas-task-step-text" />
                  </div>
                )}
                {(m.links.length > 0 || m.iteration != null) && (
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 5, marginTop: 6 }}>
                    {m.links.map((l) => (
                      <span key={l} style={{ fontSize: "0.64em", fontFamily: "var(--ct-font-mono)", padding: "0.05em 0.4em", borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg-active)", color: "var(--ct-fg)", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{l}</span>
                    ))}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    );
  }

  // W0/W1/W2 — the SVG snake. Geometry scales with the band.
  const cfg = !atLeast(world, "W1")
    ? { r: 3.5, pitch: 16, rowH: 14, topPad: 8, cap: 10, labels: false as const, callouts: false as const }
    : !atLeast(world, "W2")
      ? { r: 6, pitch: 30, rowH: 34, topPad: 18, cap: 64, labels: true as const, callouts: false as const }
      : { r: 9, pitch: 46, rowH: 52, topPad: 26, cap: 64, labels: false as const, callouts: true as const };

  const shown = milestones.slice(0, cfg.cap);
  const extra = milestones.length - shown.length;
  const w = Math.max(80, width);
  const pad = cfg.r + 6;
  const { beads, height } = layoutBeads(shown.length, w, pad, cfg.pitch, cfg.rowH, cfg.topPad);
  const path = snakePath(beads, cfg.r);
  const mode = !atLeast(world, "W1") ? "constellation" : !atLeast(world, "W2") ? "thread" : "cards";

  return (
    <div data-testid="canvas-task-steps" data-step-mode={mode} style={{ position: "relative", width: w, maxWidth: "100%" }}>
      <svg width={w} height={height} viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="xMinYMin meet" style={{ overflow: "visible", display: "block" }}>
        <path d={path} fill="none" stroke="color-mix(in srgb, var(--ct-fg) 26%, transparent)" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
        {beads.map((b, i) => {
          const m = shown[i];
          const color = colorFor(m.kind, colors);
          const isFresh = m.id === fresh;
          return (
            <Fragment key={m.id}>
              {atLeast(world, "W1") && (
                <circle cx={b.x} cy={b.y} r={cfg.r + 2.5} fill="none" stroke={color} strokeWidth={1.5} opacity={0.55} />
              )}
              <circle data-testid="canvas-task-step-dot" data-milestone-id={m.id} className={isFresh ? "ct-canvas-step-live" : undefined} cx={b.x} cy={b.y} r={cfg.r} fill={color} style={{ transformOrigin: `${b.x}px ${b.y}px` }} />
            </Fragment>
          );
        })}
      </svg>
      {/* W1 labels alternate above/below each bead; W2 callouts sit on the line. */}
      {(cfg.labels || cfg.callouts) && beads.map((b, i) => {
        const m = shown[i];
        const above = b.row % 2 === 0;
        const label = digestMarkdown(m.text, cfg.callouts ? 64 : 24) || m.kind.replace(/_/g, " ");
        const lw = cfg.callouts ? cfg.pitch * 1.7 : cfg.pitch - 4;
        return (
          <div
            key={`l-${m.id}`}
            data-testid="canvas-stride-label"
            style={{
              position: "absolute",
              left: b.x,
              top: b.y + (above ? -cfg.r - 13 : cfg.r + 3),
              transform: "translateX(-50%)",
              width: lw,
              fontSize: cfg.callouts ? "0.72em" : "0.62em",
              lineHeight: 1.15,
              textAlign: "center",
              whiteSpace: cfg.callouts ? "normal" : "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              color: "var(--ct-fg)",
              pointerEvents: "none",
            }}
          >
            {label}
          </div>
        );
      })}
      {extra > 0 && (
        <span style={{ position: "absolute", right: 0, top: 0, fontSize: "0.62em", fontWeight: 700, color: "var(--ct-fg)" }}>+{extra}</span>
      )}
    </div>
  );
}

function miniBtn(): React.CSSProperties {
  return { display: "inline-flex", alignItems: "center", justifyContent: "center", width: 18, height: 18, fontSize: 11, lineHeight: 1, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 };
}
