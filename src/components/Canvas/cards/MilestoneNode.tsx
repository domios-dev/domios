import type { CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Milestone } from "../types";
import type { CanvasCardStyleProps } from "./types";

const nodeStyle: CSSProperties = {
  boxSizing: "border-box",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  textAlign: "center",
  fontFamily: "var(--ct-font-ui)",
  color: "var(--ct-fg)",
};

export interface MilestoneNodeProps extends CanvasCardStyleProps {
  milestone: Milestone;
}

export function MilestoneNode({ milestone, style, lod = "near" }: MilestoneNodeProps) {
  const size = lod === "far" ? 28 : lod === "mid" ? 36 : 48;

  return (
    <div
      data-testid="canvas-milestone-node"
      data-milestone-id={milestone.id}
      style={{
        ...nodeStyle,
        ...style,
        width: size,
        minHeight: size,
        borderRadius: "50%",
        border: "2px solid var(--ct-border-focus)",
        background: "var(--ct-bg)",
        padding: lod === "near" ? 6 : 4,
        fontSize: "var(--ct-font-size-sm)",
      }}
    >
      <span
        data-testid="canvas-milestone-kind"
        style={{
          fontWeight: 600,
          fontSize: lod === "far" ? "0.65rem" : "var(--ct-font-size-sm)",
          lineHeight: 1.2,
          textTransform: "capitalize",
        }}
      >
        {milestone.kind.replace(/_/g, " ")}
      </span>
      {lod === "near" && milestone.text.trim().length > 0 && (
        <div
          data-testid="canvas-milestone-text"
          style={{
            marginTop: 4,
            maxWidth: 120,
            fontSize: "0.7rem",
            opacity: "var(--ct-text-secondary-opacity)",
          }}
        >
          <RichMarkdown text={milestone.text} />
        </div>
      )}
      {milestone.iteration != null && lod !== "far" && (
        <span
          data-testid="canvas-milestone-iteration"
          style={{
            position: "absolute",
            top: -4,
            right: -4,
            minWidth: 18,
            height: 18,
            padding: "0 4px",
            borderRadius: 9,
            background: "var(--ct-border-focus)",
            color: "var(--ct-bg)",
            fontSize: "0.65rem",
            fontWeight: 700,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {milestone.iteration}
        </span>
      )}
    </div>
  );
}
