import type { CSSProperties } from "react";
import { FileText } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan, Section } from "../types";
import type { CanvasCardStyleProps } from "./types";

// A RECESSED reference doc — not the hero. Surface/titlebar tone (never paper-
// white), quiet chrome, no shadow so the compact plan node reads as the anchor.
const shell: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-subtle)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-titlebar-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "none",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
  opacity: 0.96,
};

function SpecSection({ section, depth }: { section: Section; depth: number }) {
  return (
    <div style={{ marginTop: depth === 0 ? 13 : 9, paddingLeft: depth * 11 }}>
      <div
        data-testid="canvas-spec-heading"
        role="heading"
        aria-level={Math.min(6, depth + 3)}
        style={{
          margin: 0,
          fontSize: depth === 0 ? "var(--ct-font-size-md)" : "0.74rem",
          fontWeight: 700,
          letterSpacing: depth === 0 ? "-0.01em" : "0.03em",
          textTransform: depth === 0 ? "none" : "uppercase",
          color: depth === 0 ? "var(--ct-fg)" : "var(--ct-border-focus)",
        }}
      >
        {section.title}
      </div>
      {section.body.trim().length > 0 && (
        <div
          style={{
            marginTop: 4,
            fontSize: "var(--ct-font-size-sm)",
            color: "var(--ct-fg)",
            opacity: 0.82,
            lineHeight: 1.55,
          }}
        >
          <RichMarkdown text={section.body} />
        </div>
      )}
      {section.subsections.map((sub) => (
        <SpecSection key={sub.id} section={sub} depth={depth + 1} />
      ))}
    </div>
  );
}

export interface SpecNodeProps extends CanvasCardStyleProps {
  plan: Plan;
}

export function SpecNode({ plan, style }: SpecNodeProps) {
  return (
    <div data-testid="canvas-spec-node" data-plan-id={plan.id} style={{ ...shell, ...style }}>
      <div
        style={{
          padding: "9px 16px",
          borderBottom: "1px solid var(--ct-border-subtle)",
          display: "flex",
          alignItems: "center",
          gap: 8,
          flexShrink: 0,
        }}
      >
        <FileText size={13} style={{ opacity: 0.55, flexShrink: 0 }} />
        <span style={{ fontWeight: 600, fontSize: "var(--ct-font-size-sm)", letterSpacing: "0.04em", textTransform: "uppercase", opacity: "var(--ct-text-secondary-opacity)" }}>
          Specification
        </span>
        <span style={{ marginLeft: "auto", fontSize: "0.64rem", opacity: "var(--ct-text-tertiary-opacity)", fontFamily: "var(--ct-font-mono)" }}>
          {plan.sections.length} §
        </span>
      </div>
      <div
        data-testid="canvas-spec-body"
        className="ct-canvas-scroll"
        style={{ flex: 1, overflowY: "auto", padding: "2px 14px 16px 16px", marginLeft: 8, borderLeft: "1px dashed var(--ct-border-subtle)" }}
        onWheel={(e) => e.stopPropagation()}
      >
        <div style={{ fontSize: "var(--ct-font-size-lg, 15px)", fontWeight: 700, letterSpacing: "-0.01em", marginTop: 12, marginBottom: 2 }}>
          {plan.title}
        </div>
        {plan.sections.map((section) => (
          <SpecSection key={section.id} section={section} depth={0} />
        ))}
      </div>
    </div>
  );
}
