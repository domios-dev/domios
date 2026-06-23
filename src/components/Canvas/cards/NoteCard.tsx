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
  return typeof element.content === "string" ? element.content : "";
}

export interface NoteCardProps extends CanvasCardStyleProps {
  element: CanvasElement;
}

export function NoteCard({ element, style, lod = "near" }: NoteCardProps) {
  const text = elementMarkdown(element);

  return (
    <div
      data-testid="canvas-note-card"
      data-element-id={element.id}
      style={{ ...cardStyle, ...style }}
    >
      {lod === "near" && text.trim().length > 0 && (
        <div data-testid="canvas-note-body" style={{ fontSize: "var(--ct-font-size-sm)" }}>
          <RichMarkdown text={text} />
        </div>
      )}
      {lod !== "near" && text.trim().length > 0 && (
        <div
          data-testid="canvas-note-preview"
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
