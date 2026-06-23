import { useState, type CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Milestone } from "../types";
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
}

// Circular milestone bubble (§2.4): colored by kind lane, hover reveals the
// kind label + markdown text, iteration badge stacks. Sized to its layout box.
export function MilestoneBubble({ model, style, lod = "near" }: MilestoneBubbleProps) {
  const { milestone, color: rawColor } = model;
  // Desaturate ~18% so bubbles read as supporting detail, not the loudest thing.
  const color = `color-mix(in srgb, ${rawColor} 82%, var(--ct-muted-fg, #94a3b8))`;
  const [hover, setHover] = useState(false);
  const showTip = hover || lod === "near";
  const kindLabel = milestone.kind.replace(/_/g, " ");

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
          fontSize: lod === "far" ? "0.8rem" : "0.95rem",
          lineHeight: 1,
          color: "var(--ct-fg)",
          opacity: 0.92,
          textShadow: "0 1px 2px color-mix(in srgb, var(--ct-bg) 60%, transparent)",
        }}
      >
        {kindLabel.charAt(0).toUpperCase()}
      </span>
      {milestone.iteration != null && lod !== "far" && (
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
            pointerEvents: hover ? "auto" : "none",
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
              marginBottom: milestone.text.trim() ? 3 : 0,
            }}
          >
            <span style={{ width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />
            {kindLabel}
          </div>
          {milestone.text.trim().length > 0 && <RichMarkdown text={milestone.text} />}
        </div>
      )}
    </div>
  );
}
