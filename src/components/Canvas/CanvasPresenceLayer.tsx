import { memo, useMemo } from "react";
import { AgentStack } from "./presence/AgentCircle";
import type { PresenceOverlay } from "./presenceOverlays";
import { usePresenceGlide } from "./presenceGlide";
import { atLeast } from "./worlds";

// EMERGENCE presence layer: pins live in WORLD space inside the camera-transformed
// layer (layerRef) so connectors + avatars pan/zoom in lockstep with task nodes —
// never a screen overlay that trails behind a rAF canvas blit. Avatars scale with
// zoom like the board; the container is pointer-events:none; only pins capture clicks.

export interface CanvasPresenceLayerProps {
  overlays: readonly PresenceOverlay[];
  onFlyToTask: (taskId: string) => void;
}

// Offset from the task corner in world units (scales with zoom like the card).
const FLOAT_X = 32;
const FLOAT_Y = -28;

function rosterTitle(overlay: PresenceOverlay): string {
  return `Working: ${overlay.agents.map((a) => a.label + (a.role ? ` (${a.role})` : "")).join(", ")}`;
}

export const CanvasPresenceLayer = memo(function CanvasPresenceLayer({ overlays, onFlyToTask }: CanvasPresenceLayerProps) {
  const items = useMemo(() => overlays, [overlays]);
  const { pinRef } = usePresenceGlide(items);
  if (items.length === 0) return null;

  return (
    <div
      data-testid="canvas-presence-layer"
      data-presence-count={items.length}
      className="pointer-events-none absolute inset-0"
      style={{ overflow: "visible" }}
    >
      {items.map((overlay) => {
        const c = overlay.agents[0]?.color ?? "var(--ct-border-focus)";
        return (
          <svg
            key={`presence-link-${overlay.taskId}`}
            data-testid="canvas-presence-link"
            data-task-id={overlay.taskId}
            className="pointer-events-none absolute overflow-visible"
            style={{ left: overlay.anchor.x, top: overlay.anchor.y, overflow: "visible" }}
            width={1}
            height={1}
          >
            <line
              x1={FLOAT_X}
              y1={FLOAT_Y}
              x2={0}
              y2={0}
              stroke={c}
              strokeWidth={1.25}
              strokeDasharray="3 4"
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
              opacity={overlay.live ? 0.7 : 0.38}
            />
            <circle cx={0} cy={0} r={2.5} fill={c} vectorEffect="non-scaling-stroke" opacity={overlay.live ? 0.85 : 0.5} />
          </svg>
        );
      })}
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
                        {overlay.action}
                      </span>
                    )}
                  </span>
                )}
              </span>
            ) : (
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
