import { memo, useMemo, useState } from "react";
import { AgentStack } from "./presence/AgentCircle";
import type { PresenceOverlay } from "./presenceOverlays";
import { livePresencePins } from "./presencePinUnit";
import { usePresenceGlide } from "./presenceGlide";
import { shouldRevealPresenceDetails } from "./presenceReveal";
import { atLeast } from "./worlds";

export interface CanvasPresenceLayerProps {
  overlays: readonly PresenceOverlay[];
  emphasisTaskIds?: ReadonlySet<string>;
  onFlyToTask: (taskId: string) => void;
}

const FLOAT_X = 32;
const FLOAT_Y = -28;

function rosterTitle(overlay: PresenceOverlay): string {
  return `Working: ${overlay.agents.map((a) => a.label + (a.role ? ` (${a.role})` : "")).join(", ")}`;
}

function PresencePin({
  overlay,
  pinRef,
  emphasisTaskIds,
  onFlyToTask,
}: {
  overlay: PresenceOverlay;
  pinRef: (taskId: string) => (el: HTMLElement | null) => void;
  emphasisTaskIds: ReadonlySet<string>;
  onFlyToTask: (taskId: string) => void;
}) {
  const [pinHovered, setPinHovered] = useState(false);
  const lead = overlay.agents[0];
  if (!lead) return null;

  const reveal = shouldRevealPresenceDetails(overlay.world, overlay.taskId, emphasisTaskIds, pinHovered);
  const showAvatar = atLeast(overlay.world, "W1");
  const size = atLeast(overlay.world, "W2") ? 20 : 16;
  const color = lead.color;
  const liveRing = overlay.live && reveal;

  return (
    <div
      style={{ position: "absolute", left: overlay.anchor.x, top: overlay.anchor.y, pointerEvents: "none" }}
      data-testid="canvas-presence-pin"
      data-task-id={overlay.taskId}
      data-presence-reveal={reveal ? "true" : "false"}
      data-stale={overlay.stale ? "true" : "false"}
    >
      <span
        ref={pinRef(overlay.taskId)}
        style={{ display: "block", position: "relative", lineHeight: 0, willChange: "transform" }}
      >
        {reveal && (
          <svg
            data-testid="canvas-presence-link"
            className="pointer-events-none absolute overflow-visible"
            style={{ left: 0, top: 0, overflow: "visible" }}
            width={1}
            height={1}
            aria-hidden
          >
            <line
              x1={FLOAT_X}
              y1={FLOAT_Y}
              x2={0}
              y2={0}
              stroke={color}
              strokeWidth={1.25}
              strokeDasharray="3 4"
              strokeLinecap="round"
              opacity={overlay.live ? 0.7 : 0.38}
            />
            <circle cx={0} cy={0} r={2.5} fill={color} opacity={overlay.live ? 0.85 : 0.5} />
          </svg>
        )}
        <button
          type="button"
          data-testid="canvas-presence-overlay"
          data-presence-world={overlay.world}
          data-presence-live={overlay.live ? "true" : "false"}
          data-stale={overlay.stale ? "true" : "false"}
          title={rosterTitle(overlay)}
          onMouseEnter={() => setPinHovered(true)}
          onMouseLeave={() => setPinHovered(false)}
          onClick={(e) => {
            e.stopPropagation();
            onFlyToTask(overlay.taskId);
          }}
          style={{
            position: "absolute",
            left: FLOAT_X,
            top: FLOAT_Y,
            transform: "translate(-50%, -50%)",
            pointerEvents: "auto",
            display: "flex",
            alignItems: "center",
            padding: 0,
            border: "none",
            background: "transparent",
            cursor: "pointer",
            lineHeight: 0,
          }}
        >
          {showAvatar ? (
            <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
              <AgentStack agents={overlay.agents} size={size} live={liveRing} />
              {reveal && (
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
                    border: `1px solid color-mix(in srgb, ${color} 55%, transparent)`,
                    boxShadow: "0 1px 4px rgba(0, 0, 0, 0.22)",
                  }}
                >
                  <span style={{ maxWidth: 168, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", fontSize: 11, fontWeight: 700, lineHeight: 1.2 }}>
                    {overlay.label}
                  </span>
                  {overlay.action && (
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
              className={liveRing ? "ct-presence-live" : overlay.stale ? "ct-presence-stale" : undefined}
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                background: color,
                boxShadow: `0 0 0 1px color-mix(in srgb, ${color} 60%, transparent)`,
                ["--ct-presence-color" as string]: color,
              }}
            />
          )}
        </button>
      </span>
    </div>
  );
}

export const CanvasPresenceLayer = memo(function CanvasPresenceLayer({
  overlays,
  emphasisTaskIds = new Set(),
  onFlyToTask,
}: CanvasPresenceLayerProps) {
  const items = useMemo(() => livePresencePins(overlays), [overlays]);
  const { pinRef } = usePresenceGlide(items);
  if (items.length === 0) return null;

  return (
    <div
      data-testid="canvas-presence-layer"
      data-presence-count={items.length}
      className="pointer-events-none absolute inset-0"
      style={{ overflow: "visible" }}
    >
      {items.map((overlay) => (
        <PresencePin
          key={overlay.taskId}
          overlay={overlay}
          pinRef={pinRef}
          emphasisTaskIds={emphasisTaskIds}
          onFlyToTask={onFlyToTask}
        />
      ))}
    </div>
  );
});
