import type { CSSProperties, ReactNode } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import {
  MilestoneBubble,
  PlanNode,
  SpecSectionCard,
  TaskCard,
  type CanvasLod,
  type MilestoneBubbleModel,
} from "./cards";
import type { LaidOutNode } from "./layout";
import { registerProvenanceRenderers } from "./registerProvenanceRenderers";
import type { SectionRowGlyph as SpecGlyph } from "./sectionRows";
import type { CanvasElement, Plan, Section, Task } from "./types";

// kind → renderer dispatch. Adding a node kind = one register call; canvas core,
// layout, pan/zoom and focus detection stay untouched. Open-kind fallback kept.
export interface NodeRenderArgs {
  node: LaidOutNode;
  model: unknown;
  style: CSSProperties;
  lod: CanvasLod;
  onArchive?: () => void;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
}

export type NodeRenderer = (args: NodeRenderArgs) => ReactNode;

const renderers: Record<string, NodeRenderer> = {};

export function registerNodeRenderer(kind: string, renderer: NodeRenderer): void {
  renderers[kind] = renderer;
}

const fallbackStyle: CSSProperties = {
  position: "absolute",
  boxSizing: "border-box",
  border: "1px dashed var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  padding: "10px 12px",
  boxShadow: "var(--ct-shadow-sm)",
};

export const fallbackRenderer: NodeRenderer = ({ node, model, style }) => {
  const el = model as CanvasElement | undefined;
  const content = typeof el?.content === "string" ? el.content : null;
  return (
    <div data-testid="canvas-fallback-node" data-node-kind={node.kind} style={{ ...fallbackStyle, ...style }}>
      <div style={{ fontWeight: 600, fontSize: "var(--ct-font-size-sm)", textTransform: "capitalize" }}>
        {node.kind.replace(/_/g, " ")}
      </div>
      {content && (
        <div style={{ fontSize: "var(--ct-font-size-sm)", marginTop: 4 }}>
          <RichMarkdown text={content} />
        </div>
      )}
    </div>
  );
};

export function getNodeRenderer(kind: string): NodeRenderer {
  return renderers[kind] ?? fallbackRenderer;
}

registerNodeRenderer("plan", ({ model, style, lod, onArchive }) =>
  model ? <PlanNode plan={model as Plan} style={style} lod={lod} onArchive={onArchive} /> : null,
);
registerNodeRenderer("spec-section", ({ model, style }) => {
  const m = model as { section: Section; glyph?: SpecGlyph; taskCount?: number } | undefined;
  return m ? <SpecSectionCard section={m.section} glyph={m.glyph} taskCount={m.taskCount} style={style} /> : null;
});
registerNodeRenderer("task", ({ model, style, onOpenPane, onHoverTask }) => {
  const m = model as { task: Task; childCount?: number } | undefined;
  return m ? <TaskCard task={m.task} childCount={m.childCount} style={style} onOpenPane={onOpenPane} onHover={onHoverTask} /> : null;
});
// Soft container behind a parent task's nested subtask children.
registerNodeRenderer("subtask-group", ({ style }) => (
  <div
    data-testid="canvas-subtask-group"
    style={{
      ...style,
      position: "absolute",
      borderRadius: "var(--ct-radius-md)",
      background: "color-mix(in srgb, var(--ct-fg) 3%, transparent)",
      border: "1px dashed var(--ct-border-subtle)",
      pointerEvents: "none",
    }}
  />
));
registerNodeRenderer("milestone", ({ model, style, lod }) =>
  model ? <MilestoneBubble model={model as MilestoneBubbleModel} style={style} lod={lod} /> : null,
);
// Faint enclosure behind a task's step-bubble cluster + a kind caption row.
registerNodeRenderer("cluster", ({ model, style }) => {
  const kinds = (model as { kinds?: { label: string; color: string }[] } | undefined)?.kinds ?? [];
  return (
    <div
      data-testid="canvas-cluster-bg"
      style={{
        ...style,
        position: "absolute",
        borderRadius: "var(--ct-radius-md)",
        background: "color-mix(in srgb, var(--ct-fg) 3.5%, transparent)",
        border: "1px solid var(--ct-border-subtle)",
        pointerEvents: "none",
        fontFamily: "var(--ct-font-ui)",
      }}
    >
      {kinds.length > 0 && (
        <div
          data-testid="canvas-cluster-caption"
          style={{
            display: "flex",
            alignItems: "center",
            gap: 5,
            padding: "2px 7px 0",
            height: 17,
            fontSize: "0.6rem",
            fontWeight: 700,
            textTransform: "uppercase",
            letterSpacing: "0.03em",
            whiteSpace: "nowrap",
            overflow: "hidden",
            color: "var(--ct-fg)",
            opacity: 0.7,
          }}
        >
          {kinds.slice(0, 3).map((k) => (
            <span key={k.label} style={{ display: "inline-flex", alignItems: "center", gap: 3, minWidth: 0 }}>
              <span style={{ width: 6, height: 6, borderRadius: "50%", background: k.color, flexShrink: 0 }} />
              <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{k.label}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
});

registerProvenanceRenderers(registerNodeRenderer);
