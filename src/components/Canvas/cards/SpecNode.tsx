import type { CSSProperties } from "react";
import { FileText } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan, Section } from "../types";
import type { CanvasCardStyleProps } from "./types";

const shell: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-lg)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  boxShadow: "var(--ct-shadow-md)",
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
};

function SpecSection({ section, depth }: { section: Section; depth: number }) {
  return (
    <div style={{ marginTop: depth === 0 ? 14 : 10, paddingLeft: depth * 12 }}>
      <div
        data-testid="canvas-spec-heading"
        role="heading"
        aria-level={Math.min(6, depth + 3)}
        style={{
          margin: 0,
          fontSize: depth === 0 ? "var(--ct-font-size-md)" : "var(--ct-font-size-sm)",
          fontWeight: 700,
          letterSpacing: "-0.01em",
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
            opacity: "var(--ct-text-secondary-opacity)",
            lineHeight: 1.5,
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
          padding: "10px 14px",
          borderBottom: "1px solid var(--ct-border-subtle)",
          display: "flex",
          alignItems: "center",
          gap: 8,
          flexShrink: 0,
          background: "var(--ct-titlebar-bg)",
        }}
      >
        <FileText size={14} style={{ opacity: 0.7, flexShrink: 0 }} />
        <span style={{ fontWeight: 700, fontSize: "var(--ct-font-size-md)", letterSpacing: "-0.01em" }}>
          {plan.title}
        </span>
        <span style={{ marginLeft: "auto", fontSize: "0.68rem", opacity: "var(--ct-text-tertiary-opacity)" }}>
          spec
        </span>
      </div>
      <div
        data-testid="canvas-spec-body"
        className="ct-scroll"
        style={{ flex: 1, overflowY: "auto", padding: "0 14px 16px" }}
        onWheel={(e) => e.stopPropagation()}
      >
        {plan.sections.map((section) => (
          <SpecSection key={section.id} section={section} depth={0} />
        ))}
      </div>
    </div>
  );
}
