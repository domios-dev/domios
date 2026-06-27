import { useState, type CSSProperties } from "react";
import type { Milestone } from "../types";
import { MilestoneText } from "./MilestoneText";
import { ownerName } from "../owner";
import { formatTime } from "../../../lib/formatters";
import { atLeast, type World } from "../worlds";
import type { CanvasCardStyleProps } from "./types";

export interface MilestoneBubbleModel {
  milestone: Milestone;
  color: string;
}

const base: CSSProperties = {
  boxSizing: "border-box",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  textAlign: "center",
  fontFamily: "var(--ct-font-ui)",
  color: "var(--ct-fg)",
  borderRadius: "50%",
  cursor: "pointer",
};

export interface MilestoneBubbleProps extends CanvasCardStyleProps {
  model: MilestoneBubbleModel;
  /** §26 worlds: at W3 the detail panel pins open so diving in reads the stride. */
  world?: World;
}

// Circular milestone bubble (§2.4): colored by kind lane. Hover (or diving to W3)
// reveals the kind label + markdown text + provenance. Sized to its layout box.
export function MilestoneBubble({ model, style, world }: MilestoneBubbleProps) {
  const { milestone, color: rawColor } = model;
  // Desaturate ~18% so bubbles read as supporting detail, not the loudest thing.
  const color = `color-mix(in srgb, ${rawColor} 82%, var(--ct-muted-fg, #94a3b8))`;
  const [hover, setHover] = useState(false);
  // W3 deep-dive PINS the detail open (no hover needed): approaching a working
  // point should reveal what it is about. Far bands stay hover-only so a dense
  // cluster doesn't pile overlapping tooltips. W2 names the kind on the face.
  const deep = atLeast(world ?? "W0", "W3");
  const showTip = hover || deep;
  const showKindFace = atLeast(world ?? "W0", "W2");
  const kindLabel = milestone.kind.replace(/_/g, " ");
  const who = ownerName(milestone.from);
  const when = milestone.ts > 0 ? formatTime(milestone.ts) : "";

  return (
    <div
      data-testid="canvas-milestone-bubble"
      data-milestone-id={milestone.id}
      data-milestone-kind={milestone.kind}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        ...base,
        ...style,
        background: `radial-gradient(circle at 35% 30%, color-mix(in srgb, ${color} 44%, var(--ct-bg)), color-mix(in srgb, ${color} 17%, var(--ct-bg)))`,
        border: `2px solid color-mix(in srgb, ${color} 85%, transparent)`,
        boxShadow: hover ? `0 0 0 4px color-mix(in srgb, ${color} 25%, transparent)` : "var(--ct-shadow-sm)",
        transition: "box-shadow var(--ct-duration-fast) var(--ct-ease-decel)",
        position: style?.position ?? "absolute",
      }}
    >
      <span
        data-testid="canvas-milestone-initial"
        aria-hidden
        style={{
          fontWeight: 800,
          fontSize: "0.95rem",
          lineHeight: 1,
          color: "var(--ct-fg)",
          opacity: 0.92,
          textShadow: "0 1px 2px color-mix(in srgb, var(--ct-bg) 60%, transparent)",
        }}
      >
        {kindLabel.charAt(0).toUpperCase()}
      </span>
      {showKindFace && (
        <span
          data-testid="canvas-milestone-kind-face"
          style={{
            marginTop: 2,
            maxWidth: "86%",
            fontSize: "0.5rem",
            fontWeight: 700,
            letterSpacing: "0.02em",
            textTransform: "capitalize",
            lineHeight: 1.1,
            opacity: 0.85,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            textShadow: "0 1px 2px color-mix(in srgb, var(--ct-bg) 60%, transparent)",
          }}
        >
          {kindLabel}
        </span>
      )}
      {milestone.iteration != null && (
        <span
          data-testid="canvas-milestone-iteration"
          style={{
            position: "absolute",
            top: -5,
            right: -5,
            minWidth: 17,
            height: 17,
            padding: "0 4px",
            borderRadius: 9,
            background: color,
            color: "var(--ct-bg)",
            fontSize: "0.6rem",
            fontWeight: 800,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1.5px solid var(--ct-bg)",
          }}
        >
          {milestone.iteration}
        </span>
      )}
      {showTip && (
        <div
          data-testid="canvas-milestone-text"
          onWheel={(e) => e.stopPropagation()}
          style={{
            position: "absolute",
            top: "calc(100% + 6px)",
            left: "50%",
            transform: "translateX(-50%)",
            minWidth: 130,
            maxWidth: 230,
            zIndex: 5,
            padding: "7px 10px",
            borderRadius: "var(--ct-radius-md)",
            border: `1px solid color-mix(in srgb, ${color} 60%, var(--ct-border-default))`,
            background: "var(--ct-bg)",
            boxShadow: "var(--ct-shadow-md)",
            fontSize: "0.72rem",
            textAlign: "left",
            opacity: 1,
            pointerEvents: hover || deep ? "auto" : "none",
          }}
        >
          <div
            data-testid="canvas-milestone-kind-label"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontWeight: 700,
              textTransform: "capitalize",
              marginBottom: milestone.text.trim() ? 4 : 0,
            }}
          >
            <span style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />
            {kindLabel}
          </div>
          {milestone.text.trim().length > 0 && (
            <div
              data-testid="canvas-milestone-detail"
              onWheel={(e) => e.stopPropagation()}
              style={{
                color: "var(--ct-fg)",
                opacity: 0.92,
                lineHeight: 1.45,
                maxHeight: 220,
                overflowY: "auto",
                whiteSpace: "normal",
                overflowWrap: "anywhere",
                wordBreak: "break-word",
              }}
            >
              <MilestoneText text={milestone.text} testId="canvas-milestone-detail-text" />
            </div>
          )}
          {milestone.links.length > 0 && (
            <div data-testid="canvas-milestone-links" style={{ display: "flex", flexWrap: "wrap", gap: 5, marginTop: 6 }}>
              {milestone.links.map((l) => (
                <span key={l} style={{ fontSize: "0.86em", fontFamily: "var(--ct-font-mono)", padding: "0.05em 0.4em", borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg-active)", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{l}</span>
              ))}
            </div>
          )}
          {(who || when || milestone.iteration != null || milestone.loop_id) && (
            <div
              data-testid="canvas-milestone-meta"
              style={{
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: 8,
                marginTop: 6,
                paddingTop: 5,
                borderTop: `1px solid color-mix(in srgb, ${color} 22%, var(--ct-border-subtle))`,
                fontSize: "0.92em",
                opacity: 0.72,
              }}
            >
              {who && <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 130 }}>{who}</span>}
              {when && <span style={{ fontFamily: "var(--ct-font-mono)" }}>{when}</span>}
              {milestone.iteration != null && <span style={{ fontFamily: "var(--ct-font-mono)" }}>#{milestone.iteration}</span>}
              {milestone.loop_id && <span title={`Loop ${milestone.loop_id}`}>↻ {milestone.loop_id}</span>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
