import type Graph from "graphology";
import { edgeKey, longEdgeFactor, mutualTopEdges, restEdgeStrength } from "./memoryEdgePolicy";
import type { MemoryGraphPalette } from "./memoryGraphTheme";
import type { SnapshotEdge } from "./snapshotTypes";

interface EdgeStyle {
  size: number;
  color: string;
  curvature: number;
  zIndex: number;
  visibility: "visible" | "hidden";
}

interface StyledEdge {
  from: string;
  to: string;
  style: EdgeStyle;
}

interface SettledView {
  ratio: number;
  width: number;
  height: number;
  project: (point: { x: number; y: number }) => { x: number; y: number };
}

const views = new WeakMap<Graph, { rest: Map<string, StyledEdge>; hovered: string | null }>();

export function settleMemoryEdges(graph: Graph, store: Map<string, SnapshotEdge[]>, palette: MemoryGraphPalette, view: SettledView, cluster: boolean) {
  const rest = new Map<string, StyledEdge>();
  const strength = cluster ? 0.08 : restEdgeStrength(view.ratio);
  if (strength === null) {
    views.set(graph, { rest, hovered: null });
    return;
  }
  const edges = cluster
    ? graph.edges().filter(key => key.startsWith("c")).map(key => ({ key, from: graph.source(key), to: graph.target(key) }))
    : mutualTopEdges([...store.values()].flat()).map(edge => ({ key: edgeKey(edge), from: edge.from, to: edge.to }));
  for (const { key, from, to } of edges) {
    if (!graph.hasNode(from) || !graph.hasNode(to)) continue;
    const source = graph.getNodeAttributes(from), target = graph.getNodeAttributes(to);
    const a = view.project({ x: source.x as number, y: source.y as number });
    const b = view.project({ x: target.x as number, y: target.y as number });
    const long = longEdgeFactor(Math.hypot(b.x - a.x, b.y - a.y), view.width, view.height) < 1;
    rest.set(key, { from, to, style: { size: 0.6, curvature: 0.15, color: palette.edgeColor(strength, long), zIndex: 0, visibility: "visible" } });
  }
  views.set(graph, { rest, hovered: null });
}

export function applyRealEdges(graph: Graph | null, _store: Map<string, SnapshotEdge[]>, _palette: MemoryGraphPalette) {
  if (!graph) return;
  for (const [key, edge] of views.get(graph)?.rest ?? []) installEdge(graph, key, edge);
}

export function applyEgoEdges(graph: Graph | null, selected: string | null, store: Map<string, SnapshotEdge[]>, palette: MemoryGraphPalette) {
  if (graph) synchronizeMemoryEgo(graph, store, palette, selected, views.get(graph)?.hovered ?? null);
}

export function synchronizeMemoryEgo(graph: Graph, store: Map<string, SnapshotEdge[]>, palette: MemoryGraphPalette, selected: string | null, hovered: string | null) {
  const view = views.get(graph) ?? { rest: new Map<string, StyledEdge>(), hovered: null };
  view.hovered = hovered;
  views.set(graph, view);
  const desired = new Map(view.rest);
  const active = selected ?? hovered;
  if (selected) {
    for (const [key, edge] of desired) {
      if (edge.from !== selected && edge.to !== selected) desired.set(key, { ...edge, style: { ...edge.style, visibility: "hidden" } });
    }
  }
  if (active) {
    for (const edge of store.get(active) ?? []) {
      desired.set(edgeKey(edge), { from: edge.from, to: edge.to, style: {
        size: selected ? 1.25 : 1, color: selected ? palette.selectedEdgeColor : palette.hoverEdgeColor,
        curvature: 0.15, visibility: "visible", zIndex: selected ? 1 : 0,
      } });
    }
  }
  for (const key of graph.edges()) if (!desired.has(key)) graph.dropEdge(key);
  for (const [key, edge] of desired) installEdge(graph, key, edge);
}

function installEdge(graph: Graph, key: string, edge: StyledEdge) {
  if (!graph.hasNode(edge.from) || !graph.hasNode(edge.to)) return;
  if (graph.hasEdge(key)) graph.mergeEdgeAttributes(key, edge.style);
  else graph.addEdgeWithKey(key, edge.from, edge.to, { ...edge.style });
}
