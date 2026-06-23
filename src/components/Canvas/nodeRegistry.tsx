import type { CSSProperties, ReactNode } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import {
  MilestoneNode,
  PlanFrame,
  SectionCard,
  TaskCard,
  type CanvasLod,
} from "./cards";
import type { LaidOutNode } from "./layout";
import type { CanvasElement, Milestone, Plan, Section, Task } from "./types";

// kind → renderer dispatch (§6.3). Adding a node kind = one register call;
// the canvas core, layout, pan/zoom and focus detection stay untouched.
export interface NodeRenderArgs {
  node: LaidOutNode;
  model: unknown;
  style: CSSProperties;
  lod: CanvasLod;
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

// Unknown kind → generic card, never crash. Renders the kind + markdown content
// when the model is a CanvasElement carrying a string payload.
export const fallbackRenderer: NodeRenderer = ({ node, model, style }) => {
  const el = model as CanvasElement | undefined;
  const content = typeof el?.content === "string" ? el.content : null;
  return (
    <div
      data-testid="canvas-fallback-node"
      data-node-kind={node.kind}
      style={{ ...fallbackStyle, ...style }}
    >
      <div
        style={{
          fontWeight: 600,
          fontSize: "var(--ct-font-size-sm)",
          textTransform: "capitalize",
        }}
      >
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

// Built-in kinds wrap the existing cards — their internals are unchanged.
registerNodeRenderer("plan", ({ model, style, lod }) =>
  model ? <PlanFrame plan={model as Plan} style={style} lod={lod} /> : null,
);
registerNodeRenderer("section", ({ model, style, lod }) =>
  model ? <SectionCard section={model as Section} style={style} lod={lod} /> : null,
);
registerNodeRenderer("task", ({ model, style, lod }) =>
  model ? <TaskCard task={model as Task} style={style} lod={lod} /> : null,
);
registerNodeRenderer("milestone", ({ model, style, lod }) =>
  model ? <MilestoneNode milestone={model as Milestone} style={style} lod={lod} /> : null,
);
