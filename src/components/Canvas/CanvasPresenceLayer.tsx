import { memo, useMemo } from "react";
import { AgentStack } from "./presence/AgentCircle";
import type { PresenceOverlay } from "./presenceOverlays";
import { usePresenceGlide } from "./presenceGlide";
import { atLeast } from "./worlds";

// EMERGENCE presence layer (north-star #2): floats the swarm ON the board. Screen-
// space (NOT inside the zoom transform) so avatars stay crisp at any zoom — like
// Figma multiplayer cursors. Each pin sits on the top-right corner of the task its
// agent executes (card body stays readable), flies to that task on click, and reads
// at its own LOD: W0 a quiet swarm-glow dot, W1 the avatar(s), W2+ avatars + a name
// flag. The container is pointer-events:none; only the pins capture clicks.

export interface CanvasPresenceLayerProps {
  overlays: readonly PresenceOverlay[];
  onFlyToTask: (taskId: string) => void;
}

// Avatar floats well OFF the task corner (screen px) so the connector reads as a real
// link (avatar → its task), not a dot stuck on the corner.
const FLOAT_X = 32;
const FLOAT_Y = -28;

function rosterTitle(overlay: PresenceOverlay): string {
  return `Working: ${overlay.agents.map((a) => a.label + (a.role ? ` (${a.role})` : "")).join(", ")}`;
}

export const CanvasPresenceLayer = memo(function CanvasPresenceLayer({ overlays, onFlyToTask }: CanvasPresenceLayerProps) {
  // Stable identity: memo the render list so unchanged pins don't reconcile churn.
  const items = useMemo(() => overlays, [overlays]);
  // EMERGENCE: pins GLIDE between tasks (Figma multiplayer) — the glide wrapper's
  // transform is written by this controller ONLY, never by React, so it can't fight
  // the camera-coupled base position or re-run any scene/layout memo.
  const { pinRef } = usePresenceGlide(items);
  if (items.length === 0) return null;

  return (
    <div
      data-testid="canvas-presence-layer"
      data-presence-count={items.length}
      className="pointer-events-none absolute inset-0"
      style={{ zIndex: 26, overflow: "hidden" }}
    >
      {/* Connectors: each worker avatar → the task it works on (drawn behind the pins).
          Same avatar+connector language as the orchestrator, one tier down: a thin
          identity-coloured dashed line vs the orchestrator's cyan link. */}
      <svg className="pointer-events-none absolute inset-0" width="100%" height="100%" style={{ overflow: "visible" }}>
        {items.map((overlay) => {
          const c = overlay.agents[0]?.color ?? "var(--ct-border-focus)";
          return (
            <g key={`presence-link-${overlay.taskId}`} data-testid="canvas-presence-link" data-task-id={overlay.taskId}>
              <line
                x1={overlay.anchor.x + FLOAT_X}
                y1={overlay.anchor.y + FLOAT_Y}
                x2={overlay.anchor.x}
                y2={overlay.anchor.y}
                stroke={c}
                strokeWidth={1.25}
                strokeDasharray="3 4"
                strokeLinecap="round"
                opacity={overlay.live ? 0.7 : 0.38}
              />
              {/* attach dot on the task end so the link clearly lands ON the task */}
              <circle cx={overlay.anchor.x} cy={overlay.anchor.y} r={2.5} fill={c} opacity={overlay.live ? 0.85 : 0.5} />
            </g>
          );
        })}
      </svg>
      {items.map((overlay) => {
        const lead = overlay.agents[0];
        const showAvatar = atLeast(overlay.world, "W1");
        const showFlag = atLeast(overlay.world, "W1");
        const showAction = atLeast(overlay.world, "W2") && !!overlay.action;
        const size = atLeast(overlay.world, "W2") ? 20 : 16;
        return (
          <button
            type="button"
            key={overlay.taskId}
            data-testid="canvas-presence-overlay"
            data-task-id={overlay.taskId}
            data-presence-world={overlay.world}
            data-presence-live={overlay.live ? "true" : "false"}
            title={rosterTitle(overlay)}
            onClick={(e) => {
              e.stopPropagation();
              onFlyToTask(overlay.taskId);
            }}
            style={{
              position: "absolute",
              left: overlay.anchor.x,
              top: overlay.anchor.y,
              // Float OFF the top-right corner (up + right) so the avatar clears the
              // card chrome and the connector back to the task reads as a real link.
              transform: `translate(calc(-50% + ${FLOAT_X}px), calc(-50% + ${FLOAT_Y}px))`,
              pointerEvents: "auto",
              display: "flex",
              alignItems: "center",
              gap: 0,
              padding: 0,
              border: "none",
              background: "transparent",
              cursor: "pointer",
              lineHeight: 0,
            }}
          >
            <span
              ref={pinRef(overlay.taskId)}
              style={{ display: "inline-flex", alignItems: "center", lineHeight: 0, willChange: "transform" }}
            >
            {showAvatar ? (
              <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
                <AgentStack agents={overlay.agents} size={size} live={overlay.live} />
                {showFlag && lead && (
                  <span
                    data-testid="canvas-presence-flag"
                    style={{
                      position: "absolute",
                      left: "100%",
                      top: "50%",
                      transform: "translateY(-50%)",
                      marginLeft: 5,
                      maxWidth: 184,
                      padding: "2px 8px",
                      borderRadius: "var(--ct-radius-sm)",
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-start",
                      gap: 1,
                      fontFamily: "var(--ct-font-ui)",
                      color: "var(--ct-fg)",
                      background: "color-mix(in srgb, var(--ct-bg) 90%, transparent)",
                      border: `1px solid color-mix(in srgb, ${lead.color} 55%, transparent)`,
                      boxShadow: "0 1px 4px color-mix(in srgb, var(--ct-shadow, #000) 22%, transparent)",
                    }}
                  >
                    <span style={{ maxWidth: 168, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", fontSize: 11, fontWeight: 700, lineHeight: 1.2 }}>
                      {overlay.label}
                    </span>
                    {showAction && (
                      <span
                        data-testid="canvas-presence-action"
                        style={{ maxWidth: 168, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", fontSize: 9.5, fontWeight: 600, lineHeight: 1.2, opacity: 0.66 }}
                      >
                        now: {overlay.action}
                      </span>
                    )}
                  </span>
                )}
              </span>
            ) : (
              // W0: a quiet swarm-glow dot — meaningful at a glance, never clutter.
              <span
                data-testid="canvas-presence-dot"
                className={overlay.live ? "ct-presence-live" : undefined}
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: lead?.color ?? "var(--ct-border-focus)",
                  boxShadow: `0 0 0 1px color-mix(in srgb, ${lead?.color ?? "var(--ct-border-focus)"} 60%, transparent)`,
                  ["--ct-presence-color" as string]: lead?.color ?? "var(--ct-border-focus)",
                }}
              />
            )}
            </span>
          </button>
        );
      })}
    </div>
  );
});
