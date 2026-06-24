import type { CSSProperties } from "react";
import { Check, CircleDot, TriangleAlert } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Section } from "../types";
import type { SectionRowGlyph } from "../sectionRows";
import type { CanvasCardStyleProps } from "./types";

// One expanded spec section, left of the plan node and edge-tied to its plan row.
// Title uses the gentle (screen-constant) curve so it reads at any zoom; body uses
// the aggressive curve and scrolls past the box. Same ready badge as the plan row.
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

export function SpecSectionCard({ section, glyph = "uncovered", taskCount = 0, style }: SpecSectionCardProps) {
  const hasBody = section.body.trim().length > 0;
  const { Icon, color } = GLYPH[glyph];
  return (
    <div data-testid="canvas-spec-section" data-section-id={section.id} data-row-glyph={glyph} className="ct-canvas-node" style={{ ...shell, ...style }}>
      <div
        style={{
          padding: "6px 12px",
          borderBottom: hasBody ? "1px solid var(--ct-border-subtle)" : "none",
          display: "flex",
          alignItems: "center",
          gap: 7,
          flexShrink: 0,
          background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)",
        }}
      >
        <Icon size={13} color={color} style={{ flexShrink: 0 }} strokeWidth={glyph === "uncovered" ? 2.4 : 2.2} />
        <span
          data-testid="canvas-spec-section-title"
          className="ct-canvas-title"
          style={{
            fontWeight: 700,
            flex: 1,
            minWidth: 0,
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
      {hasBody && (
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
