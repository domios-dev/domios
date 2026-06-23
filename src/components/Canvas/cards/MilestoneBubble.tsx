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
  const { milestone, color } = model;
  const [hover, setHover] = useState(false);
  const showText = (hover || lod === "near") && milestone.text.trim().length > 0;

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
        background: `radial-gradient(circle at 35% 30%, color-mix(in srgb, ${color} 55%, var(--ct-bg)), color-mix(in srgb, ${color} 22%, var(--ct-bg)))`,
        border: `2px solid ${color}`,
        boxShadow: hover ? `0 0 0 4px color-mix(in srgb, ${color} 25%, transparent)` : "var(--ct-shadow-sm)",
        transition: "box-shadow var(--ct-duration-fast) var(--ct-ease-decel)",
        position: style?.position ?? "absolute",
      }}
    >
      <span
        data-testid="canvas-milestone-kind-label"
        style={{
          fontWeight: 700,
          fontSize: lod === "far" ? "0.66rem" : "0.74rem",
          lineHeight: 1.05,
          textTransform: "capitalize",
          padding: "0 5px",
          maxWidth: "100%",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          textShadow: "0 1px 2px color-mix(in srgb, var(--ct-bg) 70%, transparent)",
        }}
      >
        {milestone.kind.replace(/_/g, " ")}
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
      {showText && (
        <div
          data-testid="canvas-milestone-text"
          onWheel={(e) => e.stopPropagation()}
          style={{
            position: "absolute",
            top: "calc(100% + 6px)",
            left: "50%",
            transform: "translateX(-50%)",
            minWidth: 120,
            maxWidth: 220,
            zIndex: 5,
            padding: "6px 9px",
            borderRadius: "var(--ct-radius-md)",
            border: `1px solid ${color}`,
            background: "var(--ct-bg)",
            boxShadow: "var(--ct-shadow-md)",
            fontSize: "0.72rem",
            textAlign: "left",
            opacity: hover || lod === "near" ? 1 : 0,
            pointerEvents: hover ? "auto" : "none",
          }}
        >
          <RichMarkdown text={milestone.text} />
        </div>
      )}
    </div>
  );
}
