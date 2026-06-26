import { StickyNote } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { CanvasElement } from "../types";
import { atLeast, World } from "../worlds";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { Eyebrow, bodyRegionStyle, clampStyle, firstString, provenanceCardStyle } from "./provenanceShell";

// §26 note artifact — marginalia on the plan canvas. Warm sand accent marks it as
// observational (vs the decision's firm ruling). Fixed footprint: far bands glance
// a digest (meaningful, never куцо), the deep dive reveals the full body via
// node-internal scroll — nothing truncated at W3.
const NOTE_ACCENT = "rgba(202,160,74,0.95)";

interface NoteParts {
  title: string;
  body: string;
}

function parseNote(element: CanvasElement): NoteParts {
  if (typeof element.content === "string") return { title: "", body: element.content };
  if (!element.content || typeof element.content !== "object") return { title: "", body: "" };
  const c = element.content as Record<string, unknown>;
  return {
    title: firstString(c, ["title", "heading"]),
    body: firstString(c, ["text", "body", "markdown", "note"]),
  };
}

export interface NoteCardProps extends CanvasCardStyleProps {
  element: CanvasElement;
  world: World;
}

export function NoteCard({ element, style, world }: NoteCardProps) {
  const { title, body } = parseNote(element);
  const hasBody = body.trim().length > 0;
  const deep = atLeast(world, World.W3);
  const showRich = atLeast(world, World.W2);
  const titleClamp = deep ? null : atLeast(world, World.W1) ? 3 : 2;
  const digestClamp = atLeast(world, World.W1) ? 5 : 3;
  const digest = digestMarkdown(body, 280);

  return (
    <div
      data-testid="canvas-note-card"
      data-element-id={element.id}
      data-world={world}
      className="ct-canvas-node"
      style={{ ...provenanceCardStyle(NOTE_ACCENT), padding: "9px 11px", gap: 6, ...style }}
    >
      <Eyebrow icon={<StickyNote size={12} strokeWidth={2.2} />} label="Note" accent={NOTE_ACCENT} testid="canvas-note-label" />

      {title && (
        <div
          data-testid="canvas-note-title"
          data-canvas-selectable
          className="ct-canvas-text"
          style={{ flexShrink: 0, fontSize: "0.98em", fontWeight: 700, lineHeight: 1.25, letterSpacing: "-0.01em", overflowWrap: "break-word", ...clampStyle(titleClamp) }}
        >
          {title}
        </div>
      )}

      <div className="ct-canvas-scroll" data-canvas-scroll-body={deep ? "1" : undefined} style={bodyRegionStyle(world)}>
        {!hasBody ? (
          <span style={{ fontSize: "0.85em", fontStyle: "italic", opacity: 0.45 }}>Empty note</span>
        ) : showRich ? (
          <div
            data-testid="canvas-note-body"
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ fontSize: "0.9em", lineHeight: 1.5, minHeight: 0, overflowWrap: "anywhere" }}
          >
            <RichMarkdown text={body} />
          </div>
        ) : (
          <div
            data-testid="canvas-note-body"
            data-canvas-selectable
            className="ct-canvas-text"
            style={{ fontSize: "0.88em", lineHeight: 1.45, overflowWrap: "anywhere", ...clampStyle(digestClamp) }}
          >
            {digest}
          </div>
        )}
      </div>
    </div>
  );
}
