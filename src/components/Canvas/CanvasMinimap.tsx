import type { CSSProperties } from "react";
import type { Box } from "./layout";
import type { ViewportTransform } from "./useCanvasViewport";

const MM_W = 168;
const MM_H = 120;
const PAD = 8;

const shell: CSSProperties = {
  position: "absolute",
  right: 12,
  bottom: 12,
  zIndex: 20,
  width: MM_W,
  height: MM_H,
  borderRadius: "var(--ct-radius-md)",
  border: "1px solid var(--ct-border-default)",
  background: "color-mix(in srgb, var(--ct-titlebar-bg) 92%, transparent)",
  boxShadow: "0 8px 24px -10px rgba(0,0,0,0.55)",
  backdropFilter: "blur(8px)",
  overflow: "hidden",
};

export interface CanvasMinimapProps {
  bounds: Box;
  nodes: { id: string; kind: string; box: Box }[];
  transform: ViewportTransform;
  viewport: { width: number; height: number };
}

// Minimap HUD (§23.4): the whole board framed small + a rect for the live viewport
// so you always know where the camera is on a big board. Read-only (no drag yet).
export function CanvasMinimap({ bounds, nodes, transform, viewport }: CanvasMinimapProps) {
  if (bounds.width <= 0 || bounds.height <= 0) return null;
  const scale = Math.min((MM_W - PAD * 2) / bounds.width, (MM_H - PAD * 2) / bounds.height);
  const ox = PAD - bounds.x * scale;
  const oy = PAD - bounds.y * scale;

  // Visible world rect from the camera transform.
  const vx = -transform.x / transform.scale;
  const vy = -transform.y / transform.scale;
  const vw = viewport.width / transform.scale;
  const vh = viewport.height / transform.scale;

  const color = (kind: string) =>
    kind === "plan" ? "var(--ct-border-focus)" : kind === "task" ? "rgba(148,163,184,0.9)" : "rgba(148,163,184,0.5)";

  return (
    <div data-testid="canvas-minimap" style={shell} onMouseDown={(e) => e.stopPropagation()}>
      <div style={{ position: "absolute", inset: 0 }}>
        {nodes.map((n) => (
          <div
            key={n.id}
            style={{
              position: "absolute",
              left: ox + n.box.x * scale,
              top: oy + n.box.y * scale,
              width: Math.max(2, n.box.width * scale),
              height: Math.max(2, n.box.height * scale),
              borderRadius: 1,
              background: color(n.kind),
              opacity: n.kind === "plan" ? 0.9 : 0.55,
            }}
          />
        ))}
        <div
          data-testid="canvas-minimap-viewport"
          style={{
            position: "absolute",
            left: ox + vx * scale,
            top: oy + vy * scale,
            width: vw * scale,
            height: vh * scale,
            border: "1.5px solid var(--ct-border-focus)",
            background: "color-mix(in srgb, var(--ct-border-focus) 12%, transparent)",
            borderRadius: 2,
            pointerEvents: "none",
          }}
        />
      </div>
    </div>
  );
}
