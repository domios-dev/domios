import type { CSSProperties } from "react";
import { Check, CircleDot, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Section } from "../types";
import type { SectionRowGlyph } from "../sectionRows";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";

// One spec section, three discrete view modes (§18): document = full body,
// card = title + 1–2 line digest, tile = title + glyph + count.
const shell: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-subtle)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-titlebar-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "var(--ct-shadow-sm)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
};

const GLYPH = {
  done: { Icon: Check, color: "rgba(52, 211, 153, 1)" },
  progress: { Icon: CircleDot, color: "rgba(129, 140, 248, 1)" },
  uncovered: { Icon: TriangleAlert, color: "rgba(251, 191, 36, 1)" },
} as const satisfies Record<SectionRowGlyph, { Icon: typeof Check; color: string }>;

export interface SpecSectionCardProps extends CanvasCardStyleProps {
  section: Section;
  glyph?: SectionRowGlyph;
  taskCount?: number;
}

export function SpecSectionCard({
  section,
  glyph = "uncovered",
  taskCount = 0,
  viewMode = "document",
  style,
}: SpecSectionCardProps) {
  const hasBody = section.body.trim().length > 0;
  const { Icon, color } = GLYPH[glyph];
  const tile = viewMode === "tile";
  const showBody = viewMode === "document" && hasBody;
  const showDigest = viewMode === "card" && hasBody;

  return (
    <div
      data-testid="canvas-spec-section"
      data-section-id={section.id}
      data-row-glyph={glyph}
      data-view-mode={viewMode}
      className="ct-canvas-node"
      style={{ ...shell, ...style }}
    >
      <div
        style={{
          padding: tile ? "0 12px" : "6px 12px",
          flex: tile ? 1 : "0 0 auto",
          borderBottom: showBody ? "1px solid var(--ct-border-subtle)" : "none",
          display: "flex",
          alignItems: "center",
          gap: 7,
          flexShrink: 0,
          background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)",
        }}
      >
        <Icon size={tile ? 15 : 13} color={color} style={{ flexShrink: 0 }} strokeWidth={glyph === "uncovered" ? 2.4 : 2.2} />
        <span
          data-testid="canvas-spec-section-title"
          className="ct-canvas-title"
          style={{
            fontWeight: 700,
            flex: 1,
            minWidth: 0,
            fontSize: tile ? "1.05em" : undefined,
            letterSpacing: "-0.005em",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {section.title}
        </span>
        <span
          data-testid="canvas-spec-section-count"
          style={{
            flexShrink: 0,
            minWidth: "1.4em",
            textAlign: "center",
            fontSize: "0.74em",
            fontFamily: "var(--ct-font-mono)",
            fontWeight: 600,
            padding: "0.05em 0.4em",
            borderRadius: 999,
            background: glyph === "uncovered" ? "rgba(251, 191, 36, 0.18)" : "var(--ct-bg-active)",
            color: glyph === "uncovered" ? "rgba(251, 191, 36, 0.98)" : "var(--ct-fg)",
          }}
        >
          {taskCount}
        </span>
      </div>
      {showDigest && (
        <div
          data-testid="canvas-spec-section-digest"
          style={{
            flex: 1,
            padding: "7px 12px 9px",
            fontSize: "0.9em",
            lineHeight: 1.45,
            opacity: 0.85,
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
          }}
        >
          {digestMarkdown(section.body, 180)}
        </div>
      )}
      {showBody && (
        <div
          data-testid="canvas-spec-section-body"
          className="ct-canvas-scroll"
          onWheel={(e) => e.stopPropagation()}
          style={{
            flex: 1,
            overflowY: "auto",
            padding: "7px 12px 9px",
            fontSize: "0.9em",
            color: "var(--ct-fg)",
            opacity: 0.9,
            lineHeight: 1.5,
            overflowWrap: "anywhere",
          }}
        >
          <RichMarkdown text={section.body} />
        </div>
      )}
    </div>
  );
}
