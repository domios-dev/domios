import type { CSSProperties, ReactNode } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import {
  MilestoneBubble,
  PlanNode,
  SpecNode,
  TaskCard,
  type CanvasLod,
  type MilestoneBubbleModel,
} from "./cards";
import type { LaidOutNode } from "./layout";
import { registerProvenanceRenderers } from "./registerProvenanceRenderers";
import type { CanvasElement, Plan, Task } from "./types";

// kind → renderer dispatch. Adding a node kind = one register call; canvas core,
// layout, pan/zoom and focus detection stay untouched. Open-kind fallback kept.
export interface NodeRenderArgs {
  node: LaidOutNode;
  model: unknown;
  style: CSSProperties;
  lod: CanvasLod;
  onArchive?: () => void;
  onOpenPane?: (paneId: string) => void;
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
registerNodeRenderer("spec", ({ model, style, lod }) =>
  model ? <SpecNode plan={model as Plan} style={style} lod={lod} /> : null,
);
registerNodeRenderer("task", ({ model, style, lod, onOpenPane }) =>
  model ? <TaskCard task={model as Task} style={style} lod={lod} onOpenPane={onOpenPane} /> : null,
);
registerNodeRenderer("milestone", ({ model, style, lod }) =>
  model ? <MilestoneBubble model={model as MilestoneBubbleModel} style={style} lod={lod} /> : null,
);

registerProvenanceRenderers(registerNodeRenderer);
