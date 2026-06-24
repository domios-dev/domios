import type { CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Section } from "../types";
import type { CanvasCardStyleProps } from "./types";

// One expanded spec section, left of the plan node and edge-tied to its plan row
// (§ spec-sectioned). Title + full body markdown, scrolls past its box height so
// long bodies never overflow the card.
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

export interface SpecSectionCardProps extends CanvasCardStyleProps {
  section: Section;
}

export function SpecSectionCard({ section, style, lod = "near" }: SpecSectionCardProps) {
  const hasBody = section.body.trim().length > 0;
  return (
    <div data-testid="canvas-spec-section" data-section-id={section.id} style={{ ...shell, ...style }}>
      <div
        style={{
          padding: "6px 12px",
          borderBottom: hasBody ? "1px solid var(--ct-border-subtle)" : "none",
          display: "flex",
          alignItems: "center",
          gap: 6,
          flexShrink: 0,
          background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)",
        }}
      >
        <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--ct-border-focus)", flexShrink: 0 }} />
        <span
          data-testid="canvas-spec-section-title"
          style={{
            fontWeight: 700,
            fontSize: "var(--ct-font-size-sm)",
            letterSpacing: "-0.005em",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {section.title}
        </span>
      </div>
      {hasBody && lod !== "far" && (
        <div
          data-testid="canvas-spec-section-body"
          className="ct-canvas-scroll"
          onWheel={(e) => e.stopPropagation()}
          style={{
            flex: 1,
            overflowY: "auto",
            padding: "7px 12px 9px",
            fontSize: "var(--ct-font-size-sm)",
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
