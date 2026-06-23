import type { CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { CanvasElement } from "../types";
import type { CanvasCardStyleProps } from "./types";

const cardStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderLeft: "3px solid var(--ct-accent, rgba(99, 102, 241, 0.9))",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  padding: "10px 12px",
  boxShadow: "var(--ct-shadow-sm)",
};

function elementMarkdown(element: CanvasElement): string {
  return typeof element.content === "string" ? element.content : "";
}

export interface DecisionCardProps extends CanvasCardStyleProps {
  element: CanvasElement;
}

export function DecisionCard({ element, style, lod = "near" }: DecisionCardProps) {
  const text = elementMarkdown(element);

  return (
    <div
      data-testid="canvas-decision-card"
      data-element-id={element.id}
      style={{ ...cardStyle, ...style }}
    >
      {lod !== "far" && (
        <div
          data-testid="canvas-decision-label"
          style={{
            fontSize: "var(--ct-font-size-sm)",
            fontWeight: 600,
            color: "var(--ct-accent, rgba(99, 102, 241, 0.9))",
            marginBottom: lod === "near" ? 6 : 0,
            textTransform: "uppercase",
            letterSpacing: "0.04em",
          }}
        >
          Decision
        </div>
      )}
      {lod === "near" && text.trim().length > 0 && (
        <div data-testid="canvas-decision-body" style={{ fontSize: "var(--ct-font-size-sm)" }}>
          <RichMarkdown text={text} />
        </div>
      )}
      {lod !== "near" && text.trim().length > 0 && (
        <div
          data-testid="canvas-decision-preview"
          style={{
            fontSize: "var(--ct-font-size-sm)",
            opacity: "var(--ct-text-secondary-opacity)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {text.replace(/\s+/g, " ").trim()}
        </div>
      )}
    </div>
  );
}
