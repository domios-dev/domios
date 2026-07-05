import { memo } from "react";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";
import { bezierPath, resolveEdgeEnds } from "./edges";
import type { LaidOutEdge, LaidOutNode } from "./layout";

interface EdgePaintCtx {
  hoveredTask: string | null;
  highlight: { sectionId: string; taskIds: Set<string> } | null;
  tagFilterActive: boolean;
  tagFilterMatchIds: Set<string> | null;
}

interface CanvasEdgesProps extends EdgePaintCtx {
  edges: readonly LaidOutEdge[];
  nodeById: Map<string, LaidOutNode>;
  /** Viewport cull: only paint edges touching a visible node (point-only edges always paint). */
  visibleIds?: ReadonlySet<string>;
}

function edgeJitter(id: string): number {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 1000) / 1000;
}

function paintEdge(edge: LaidOutEdge, nodeById: Map<string, LaidOutNode>, ctx: EdgePaintCtx) {
  const ends = resolveEdgeEnds(edge, nodeById);
  if (!ends) return null;
  const chain = edge.kind === "milestone-chain";
  const lineage = edge.kind === "plan-plan";
  const rowEdge = edge.kind === "section-task";
  // Spec ⊥ Plan: ONE quiet provenance link, never a focal wire — no end dots.
  const provenance = edge.kind === "spec-plan";
  const inHighlight =
    ctx.highlight != null &&
    (edge.id.startsWith(`${ctx.highlight.sectionId}~`) ||
      (edge.to != null && ctx.highlight.taskIds.has(edge.to)));
  const tagRelated = ctx.tagFilterActive && edge.to != null && ctx.tagFilterMatchIds!.has(edge.to);
  const related =
    (ctx.hoveredTask != null && (edge.from === ctx.hoveredTask || edge.to === ctx.hoveredTask)) ||
    inHighlight ||
    tagRelated;
  const dimmed = (ctx.hoveredTask != null || ctx.highlight != null || ctx.tagFilterActive) && !related;
  const j = rowEdge ? edgeJitter(edge.id) : 0.5;
  const curve = DEFAULT_CANVAS_SETTINGS.edgeCurve * (lineage ? 0.55 : provenance ? 0.5 : 0.65 + 0.8 * j);
  const baseW = lineage ? 1.8 : rowEdge ? 1.6 : provenance ? 1.1 : chain ? 1.2 : 1.4;
  const baseO = provenance ? 0.3 : lineage ? 0.5 : (rowEdge ? 0.42 : chain ? 0.3 : 0.36) * (rowEdge ? 0.82 + 0.36 * j : 1);
  const showDots = rowEdge || edge.kind === "task-cluster";
  return (
    <g key={edge.id} data-edge-kind={edge.kind}>
      <path
        d={bezierPath(ends.a, ends.b, curve)}
        fill="none"
        stroke={provenance ? "var(--ct-border-default)" : "var(--ct-border-focus)"}
        strokeWidth={related ? 2.6 : baseW}
        strokeLinecap="round"
        strokeDasharray={lineage ? "6 4" : provenance ? "5 6" : undefined}
        markerEnd={lineage ? "url(#ct-plan-lineage-arrow)" : undefined}
        opacity={related ? 0.95 : dimmed ? 0.12 : baseO}
      />
      {rowEdge && (
        <circle
          cx={ends.a.x}
          cy={ends.a.y}
          r={related ? 4.5 : 3}
          fill="var(--ct-border-focus)"
          opacity={related ? 1 : dimmed ? 0.3 : 0.75}
        />
      )}
      {showDots && (
        <circle
          cx={ends.b.x}
          cy={ends.b.y}
          r={related ? 4.5 : 3}
          fill="var(--ct-border-focus)"
          opacity={related ? 1 : dimmed ? 0.3 : 0.75}
        />
      )}
    </g>
  );
}

export const CanvasEdges = memo(function CanvasEdges({ edges, nodeById, visibleIds, hoveredTask, highlight, tagFilterActive, tagFilterMatchIds }: CanvasEdgesProps) {
  return (
    <>
      {edges.map((edge) => {
        if (visibleIds && (edge.from != null || edge.to != null)) {
          const seen = (edge.from != null && visibleIds.has(edge.from)) || (edge.to != null && visibleIds.has(edge.to));
          if (!seen) return null;
        }
        return paintEdge(edge, nodeById, { hoveredTask, highlight, tagFilterActive, tagFilterMatchIds });
      })}
    </>
  );
});
