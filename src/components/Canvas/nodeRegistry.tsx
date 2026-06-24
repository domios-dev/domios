import type { CSSProperties, ReactNode } from "react";
import { RichMarkdown } from "@codeterm/chat-ui";
import {
  MilestoneBubble,
  SectionNode,
  TaskCard,
  type CanvasLod,
  type MilestoneBubbleModel,
} from "./cards";
import { type World } from "./worlds";
import type { LaidOutNode } from "./layout";
import { registerProvenanceRenderers } from "./registerProvenanceRenderers";
import type { CanvasElement, Plan, Task } from "./types";

// kind → renderer dispatch. Adding a node kind = one register call; canvas core,
// layout, pan/zoom and focus detection stay untouched. Open-kind fallback kept.
export interface SectionNavArgs {
  planId: string;
  sectionId: string;
  variant: "plan" | "spec";
}

export interface NodeRenderArgs {
  node: LaidOutNode;
  model: unknown;
  style: CSSProperties;
  lod: CanvasLod;
  onArchive?: () => void;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onFitNode?: (nodeId: string) => void;
  /** §32.7 navigation/affordances (all user-initiated). */
  onCopyLink?: (id: string) => void;
  onCopyStrideLink?: (milestoneId: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  /** §26 worlds: this node's appW-driven content band (W0 mass → W3 deep). */
  world: World;
  /** §34 crisp text: re-rasterize factor for this node (1 = none, else camera scale). */
  crispScale: number;
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

registerNodeRenderer("plan", ({ node, model, style, world, crispScale, onArchive, onFitNode, onSectionClick, onSectionFrame }) =>
  model ? (
    <SectionNode
      variant="plan"
      plan={model as Plan}
      style={style}
      world={world}
      crispScale={crispScale}
      onArchive={onArchive}
      onFitNode={onFitNode ? () => onFitNode(node.id) : undefined}
      onSectionClick={onSectionClick}
      onSectionFrame={onSectionFrame}
    />
  ) : null,
);
registerNodeRenderer("spec", ({ model, style, world, crispScale, onSectionClick, onSectionFrame }) =>
  model ? <SectionNode variant="spec" plan={model as Plan} style={style} world={world} crispScale={crispScale} onSectionClick={onSectionClick} onSectionFrame={onSectionFrame} /> : null,
);
registerNodeRenderer("task", ({ model, style, world, onOpenPane, onHoverTask, onCopyLink, onCopyStrideLink, onFocusStride }) => {
  const m = model as { task: Task; childCount?: number; childDone?: number; stepColors?: Map<string, string> } | undefined;
  if (!m) return null;
  // §32.3: subtasks are SOLID satellite tiles swarming around their parent (packed
  // by the same cloud logic, smaller footprint) — never gated, never faded. Opacity
  // is NEVER a hierarchy/zoom signal; a subtask reads at every zoom, just smaller.
  return <TaskCard task={m.task} childCount={m.childCount} childDone={m.childDone} stepColors={m.stepColors} style={style} world={world} onOpenPane={onOpenPane} onHover={onHoverTask} onCopyLink={onCopyLink} onCopyStrideLink={onCopyStrideLink} onFocusStride={onFocusStride} />;
});
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
