import type { CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { CanvasElement } from "../types";
import type { CanvasCardStyleProps } from "./types";

const cardStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  padding: "10px 12px",
  boxShadow: "var(--ct-shadow-sm)",
};

function elementMarkdown(element: CanvasElement): string {
  if (typeof element.content === "string") return element.content;
  if (!element.content || typeof element.content !== "object") return "";
  const content = element.content as Record<string, unknown>;
  for (const key of ["text", "body", "markdown"]) {
    const value = content[key];
    if (typeof value === "string") return value;
  }
  return "";
}

export interface NoteCardProps extends CanvasCardStyleProps {
  element: CanvasElement;
}

export function NoteCard({ element, style }: NoteCardProps) {
  const text = elementMarkdown(element);

  return (
    <div
      data-testid="canvas-note-card"
      data-element-id={element.id}
      className="ct-canvas-node"
      style={{ ...cardStyle, ...style }}
    >
      {text.trim().length > 0 && (
        <div data-testid="canvas-note-body" style={{ fontSize: "0.92em" }}>
          <RichMarkdown text={text} />
        </div>
      )}
    </div>
  );
}
