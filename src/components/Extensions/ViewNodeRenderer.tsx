/**
 * Renders a plugin-authored declarative `ViewNode` tree (from `renderGlance`)
 * with themed host components. Mirrors `src-tauri/src/view/mod.rs`. Styling is
 * limited to the bounded `tone`/`spacing` tokens — never raw CSS from plugins.
 */
import type React from "react";

export type Tone = "default" | "muted" | "ok" | "warn" | "danger" | "accent";
export type Spacing = "none" | "sm" | "md" | "lg";
export interface NodeStyle {
  tone?: Tone;
  spacing?: Spacing;
}
export type NoteLevel = "info" | "warn" | "error";

export type ViewNode =
  | { kind: "text"; text: string; style?: NodeStyle }
  | { kind: "stat"; label: string; value: string; unit?: string; style?: NodeStyle }
  | { kind: "keyVal"; key: string; value: string; style?: NodeStyle }
  | { kind: "badge"; label: string; tone?: Tone }
  | { kind: "row"; children: ViewNode[]; style?: NodeStyle }
  | { kind: "stack"; children: ViewNode[]; style?: NodeStyle }
  | { kind: "divider" }
  | { kind: "note"; body: string; level?: NoteLevel }
  | { kind: "button"; label: string; action: string; style?: NodeStyle };

export interface GlanceView {
  title?: string;
  nodes: ViewNode[];
}

const TONE_COLOR: Record<Tone, string> = {
  default: "var(--ct-fg)",
  muted: "var(--ct-muted)",
  ok: "var(--ct-ok)",
  warn: "var(--ct-warn)",
  danger: "var(--ct-err)",
  accent: "var(--ct-accent, #5b8cff)",
};
const SPACING_PX: Record<Spacing, number> = { none: 0, sm: 4, md: 8, lg: 14 };

function toneColor(s?: NodeStyle, fallback: Tone = "default"): string {
  return TONE_COLOR[s?.tone ?? fallback];
}
function gap(s?: NodeStyle, fallback: Spacing = "md"): number {
  return SPACING_PX[s?.spacing ?? fallback];
}

function tonePill(tone: Tone = "default"): React.CSSProperties {
  const c = TONE_COLOR[tone];
  return {
    fontSize: 10.5,
    fontWeight: 560,
    padding: "1px 7px",
    borderRadius: 999,
    color: c,
    background: `color-mix(in srgb, ${c} 16%, transparent)`,
    border: `1px solid color-mix(in srgb, ${c} 35%, transparent)`,
    whiteSpace: "nowrap",
  };
}

export function ViewNodeRenderer({
  node,
  onAction,
}: {
  node: ViewNode;
  onAction: (action: string) => void;
}) {
  switch (node.kind) {
    case "text":
      return <div style={{ fontSize: 12.5, color: toneColor(node.style) }}>{node.text}</div>;

    case "stat":
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <div style={{ fontSize: 20, fontWeight: 640, color: toneColor(node.style), lineHeight: 1.1 }}>
            {node.value}
            {node.unit && <span style={{ fontSize: 11, color: "var(--ct-muted)", marginLeft: 3 }}>{node.unit}</span>}
          </div>
          <div style={{ fontSize: 10.5, color: "var(--ct-muted)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
            {node.label}
          </div>
        </div>
      );

    case "keyVal":
      return (
        <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12, fontSize: 12.5 }}>
          <span style={{ color: "var(--ct-muted)" }}>{node.key}</span>
          <span style={{ color: toneColor(node.style), fontWeight: 520, textAlign: "right", wordBreak: "break-all" }}>
            {node.value}
          </span>
        </div>
      );

    case "badge":
      return <span style={tonePill(node.tone)}>{node.label}</span>;

    case "row":
      return (
        <div style={{ display: "flex", alignItems: "center", gap: gap(node.style) }}>
          {node.children.map((c, i) => (
            <ViewNodeRenderer key={i} node={c} onAction={onAction} />
          ))}
        </div>
      );

    case "stack":
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: gap(node.style) }}>
          {node.children.map((c, i) => (
            <ViewNodeRenderer key={i} node={c} onAction={onAction} />
          ))}
        </div>
      );

    case "divider":
      return <div style={{ height: 1, background: "var(--ct-border-subtle, rgba(255,255,255,0.08))", margin: "2px 0" }} />;

    case "note": {
      const tone: Tone = node.level === "error" ? "danger" : node.level === "warn" ? "warn" : "muted";
      return (
        <div
          style={{
            fontSize: 11.5,
            color: TONE_COLOR[tone],
            background: `color-mix(in srgb, ${TONE_COLOR[tone]} 10%, transparent)`,
            border: `1px solid color-mix(in srgb, ${TONE_COLOR[tone]} 28%, transparent)`,
            borderRadius: 7,
            padding: "7px 9px",
          }}
        >
          {node.body}
        </div>
      );
    }

    case "button": {
      const accent = node.style?.tone === "danger" ? TONE_COLOR.danger : TONE_COLOR.accent;
      return (
        <button
          onClick={() => onAction(node.action)}
          style={{
            alignSelf: "flex-start",
            fontSize: 12,
            fontWeight: 540,
            padding: "5px 12px",
            borderRadius: 7,
            border: `1px solid color-mix(in srgb, ${accent} 40%, transparent)`,
            background: `color-mix(in srgb, ${accent} 14%, transparent)`,
            color: accent,
            cursor: "pointer",
          }}
        >
          {node.label}
        </button>
      );
    }
  }
}

export function GlanceBody({
  view,
  onAction,
}: {
  view: GlanceView;
  onAction: (action: string) => void;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {view.nodes.map((n, i) => (
        <ViewNodeRenderer key={i} node={n} onAction={onAction} />
      ))}
    </div>
  );
}
