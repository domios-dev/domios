import type { CSSProperties } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Section } from "../types";
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

function subsectionCount(sections: Section[]): number {
  let count = sections.length;
  for (const section of sections) {
    count += subsectionCount(section.subsections);
  }
  return count;
}

export interface SectionCardProps extends CanvasCardStyleProps {
  section: Section;
}

export function SectionCard({ section, style, lod = "near" }: SectionCardProps) {
  const nested = subsectionCount(section.subsections);

  return (
    <div
      data-testid="canvas-section-card"
      data-section-id={section.id}
      style={{ ...cardStyle, ...style }}
    >
      <div
        style={{
          fontWeight: 600,
          fontSize: "var(--ct-font-size-md)",
          marginBottom: lod === "far" ? 0 : 6,
        }}
      >
        {section.title}
      </div>
      {lod !== "far" && nested > 0 && (
        <div
          data-testid="canvas-section-subsections"
          style={{
            fontSize: "var(--ct-font-size-sm)",
            opacity: "var(--ct-text-secondary-opacity)",
            marginBottom: 6,
          }}
        >
          {nested} subsection{nested === 1 ? "" : "s"}
        </div>
      )}
      {lod === "near" && section.body.trim().length > 0 && (
        <div data-testid="canvas-section-body" style={{ fontSize: "var(--ct-font-size-sm)" }}>
          <RichMarkdown text={section.body} />
        </div>
      )}
    </div>
  );
}
