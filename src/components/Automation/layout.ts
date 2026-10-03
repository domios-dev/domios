import { boxesOverlap, type Box, type LaidOutNode, type Point } from "../Canvas/layout";
import { computeFitTransform, type ViewportTransform } from "../Canvas/useCanvasViewport";
import { automationKey, type AutomationEdge, type AutomationGraph } from "./types";

export const NODE_WIDTH = 232;
export const NODE_HEIGHT = 104;
/** Smallest camera scale at which the 11px card and edge text stays legible (about 9.4px on screen). */
export const READABLE_SCALE = 0.85;
export const FIT_PADDING = 28;
const SPACING = { horizontal: { layer: 168, row: 44 }, vertical: { layer: 64, row: 32 } } as const;
const PADDING = 16;
const CURVE = 0.45;
const PORT_SPREAD = 14;
export const LABEL_LINE_HEIGHT = 13;
const LABEL_PAD_Y = 4;
const LABEL_CHAR_WIDTH = 6.4;
const LABEL_LINE_CHARS = 22;
const BACK_DROP = 56;
const BACK_LANE = 22;
const SELF_LOOP = 56;

export type AutomationOrientation = "horizontal" | "vertical";

export interface AutomationEdgeRoute {
  edge: AutomationEdge;
  from: string;
  to: string;
  path: string;
  back: boolean;
  label: Box & { lines: readonly string[]; title: string };
}

export interface AutomationLayout {
  orientation: AutomationOrientation;
  nodes: LaidOutNode[];
  byId: Map<string, LaidOutNode>;
  edges: AutomationEdgeRoute[];
  bounds: Box;
  root: Box | null;
}

export type AutomationLayouts = Record<AutomationOrientation, AutomationLayout>;

interface Segment { a: Point; c1: Point; c2: Point; b: Point }

function clip(text: string): string {
  return text.length > LABEL_LINE_CHARS ? `${text.slice(0, LABEL_LINE_CHARS - 1)}…` : text;
}

function edgeLabel(edge: AutomationEdge) {
  const title = [edge.bucket, edge.handler_type, edge.handler_id, edge.result?.status, edge.evidence.kind].filter(Boolean).join(" · ");
  const lines = [edge.bucket, [edge.handler_type, edge.result?.status].filter(Boolean).join(" · ")].filter(Boolean).map(clip);
  const width = Math.ceil(Math.max(...lines.map((line) => line.length), 1) * LABEL_CHAR_WIDTH) + 14;
  return { lines, title, width, height: lines.length * LABEL_LINE_HEIGHT + LABEL_PAD_Y * 2 };
}

function curve(a: Point, b: Point): Segment {
  const dx = b.x - a.x;
  const h = Math.max(Math.abs(dx) * CURVE, 24);
  return { a, c1: { x: a.x + (dx >= 0 ? h : -h), y: a.y }, c2: { x: b.x + (dx >= 0 ? -h : h), y: b.y }, b };
}

function pointOn({ a, c1, c2, b }: Segment, t: number): Point {
  const u = 1 - t;
  return {
    x: u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x,
    y: u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y,
  };
}

function pointAtX(segment: Segment, x: number): Point {
  let best = pointOn(segment, 0.5);
  for (let step = 0; step <= 32; step++) {
    const candidate = pointOn(segment, step / 32);
    if (Math.abs(candidate.x - x) < Math.abs(best.x - x)) best = candidate;
  }
  return best;
}

function portOffsets(count: number, side: number): number[] {
  const spread = count > 1 ? Math.min(PORT_SPREAD, (side - 36) / (count - 1)) : 0;
  return Array.from({ length: count }, (_, index) => (index - (count - 1) / 2) * spread);
}

/** Classifies cycle-closing edges by deterministic DFS from scenario roots so layering never loops. */
function forwardEdges(keys: readonly string[], edges: readonly { from: number; to: number }[], roots: readonly number[]) {
  const outgoing = keys.map(() => [] as number[]);
  edges.forEach((edge, index) => outgoing[edge.from].push(index));
  const state = keys.map(() => 0);
  const back = new Set<number>();
  const order: number[] = [];
  const visit = (start: number) => {
    if (state[start]) return;
    const stack: Array<{ node: number; next: number }> = [{ node: start, next: 0 }];
    state[start] = 1;
    while (stack.length) {
      const frame = stack[stack.length - 1];
      const edgeIndex = outgoing[frame.node][frame.next++];
      if (edgeIndex === undefined) {
        state[frame.node] = 2;
        order.push(frame.node);
        stack.pop();
        continue;
      }
      const target = edges[edgeIndex].to;
      if (state[target] === 1) back.add(edgeIndex);
      else if (state[target] === 0) {
        state[target] = 1;
        stack.push({ node: target, next: 0 });
      }
    }
  };
  roots.forEach(visit);
  keys.forEach((_, index) => visit(index));
  return { back, topological: order.reverse() };
}

/** Lays layers along the flow axis (x when horizontal, y when vertical); geometry is built in flow space and transposed once. */
export function layoutAutomationGraph(graph: AutomationGraph, orientation: AutomationOrientation = "horizontal"): AutomationLayout {
  const vertical = orientation === "vertical";
  const gap = SPACING[orientation];
  const nodeMain = vertical ? NODE_HEIGHT : NODE_WIDTH;
  const nodeCross = vertical ? NODE_WIDTH : NODE_HEIGHT;
  const keys = graph.nodes.map((node) => automationKey(node.identity));
  const index = new Map(keys.map((key, position) => [key, position]));
  const routed = graph.edges.flatMap((edge) => {
    const from = index.get(automationKey(edge.source));
    const to = index.get(automationKey(edge.target));
    return from === undefined || to === undefined ? [] : [{ edge, from, to }];
  });
  const indegree = keys.map(() => 0);
  for (const edge of routed) if (edge.from !== edge.to) indegree[edge.to]++;
  const scenarioRoots = graph.scenarios.map((scenario) => index.get(automationKey(scenario.root))).filter((root): root is number => root !== undefined);
  const roots = [...scenarioRoots, ...keys.map((_, position) => position).filter((position) => indegree[position] === 0)];
  const { back, topological } = forwardEdges(keys, routed, roots);

  const layer = keys.map(() => 0);
  const parents = keys.map(() => [] as number[]);
  for (const node of topological) {
    routed.forEach((edge, edgeIndex) => {
      if (edge.from !== node || back.has(edgeIndex)) return;
      layer[edge.to] = Math.max(layer[edge.to], layer[node] + 1);
      if (!parents[edge.to].includes(node)) parents[edge.to].push(node);
    });
  }

  const columns: number[][] = [];
  keys.forEach((_, position) => (columns[layer[position]] ??= []).push(position));
  const center = keys.map(() => 0);
  for (const column of columns) {
    if (!column) continue;
    const desired = new Map(column.map((node) => {
      const above = parents[node].filter((parent) => layer[parent] < layer[node]);
      return [node, above.length ? above.reduce((sum, parent) => sum + center[parent], 0) / above.length : Number.NaN];
    }));
    const placedAbove = column.filter((node) => !Number.isNaN(desired.get(node)));
    const ordered = [
      ...placedAbove.sort((a, b) => desired.get(a)! - desired.get(b)! || a - b),
      ...column.filter((node) => Number.isNaN(desired.get(node))),
    ];
    let previous = Number.NEGATIVE_INFINITY;
    for (const node of ordered) {
      const wanted = desired.get(node);
      const position = Math.max(Number.isNaN(wanted!) ? previous + nodeCross + gap.row : wanted!, previous + nodeCross + gap.row);
      center[node] = Number.isFinite(position) ? position : 0;
      previous = center[node];
    }
    if (placedAbove.length) {
      const drift = placedAbove.reduce((sum, node) => sum + desired.get(node)! - center[node], 0) / placedAbove.length;
      for (const node of ordered) center[node] += drift;
    }
  }
  const top = Math.min(...center, 0) - nodeCross / 2;
  const boxes: Box[] = keys.map((_, position) => ({
    x: PADDING + layer[position] * (nodeMain + gap.layer),
    y: PADDING + center[position] - nodeCross / 2 - top,
    width: nodeMain,
    height: nodeCross,
  }));

  const outPorts = new Map<number, number>();
  const inPorts = new Map<number, number>();
  const assignPorts = (side: "from" | "to", ports: Map<number, number>) => {
    keys.forEach((_, node) => {
      const attached = routed.map((edge, edgeIndex) => ({ edge, edgeIndex })).filter(({ edge, edgeIndex }) => edge[side] === node && !back.has(edgeIndex));
      attached.sort((a, b) => boxes[side === "from" ? a.edge.to : a.edge.from].y - boxes[side === "from" ? b.edge.to : b.edge.from].y || a.edgeIndex - b.edgeIndex);
      portOffsets(attached.length, nodeCross).forEach((offset, position) => ports.set(attached[position].edgeIndex, offset));
    });
  };
  assignPorts("from", outPorts);
  assignPorts("to", inPorts);

  const toBox = (box: Box): Box => vertical ? { x: box.y, y: box.x, width: box.height, height: box.width } : box;
  const toPoint = (point: Point): Point => vertical ? { x: point.y, y: point.x } : point;
  const pathOf = (segment: Segment) => {
    const [a, c1, c2, b] = [segment.a, segment.c1, segment.c2, segment.b].map(toPoint);
    return `M ${a.x} ${a.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${b.x} ${b.y}`;
  };

  const placed: Box[] = [];
  let backLane = 0;
  let right = Math.max(...boxes.map((box) => box.x + box.width), PADDING);
  let bottom = Math.max(...boxes.map((box) => box.y + box.height), PADDING);
  const edges: AutomationEdgeRoute[] = routed.map(({ edge, from, to }, edgeIndex) => {
    const source = boxes[from];
    const target = boxes[to];
    const label = edgeLabel(edge);
    const labelMain = vertical ? label.height : label.width;
    const labelCross = vertical ? label.width : label.height;
    let segment: Segment;
    let anchor: Point;
    const isBack = back.has(edgeIndex) || from === to;
    if (from === to) {
      const a = { x: source.x + source.width, y: source.y + source.height / 2 - 18 };
      const b = { x: a.x, y: a.y + 36 };
      segment = { a, c1: { x: a.x + SELF_LOOP, y: a.y - 24 }, c2: { x: b.x + SELF_LOOP, y: b.y + 24 }, b };
      anchor = { x: a.x + SELF_LOOP + 4 + labelMain / 2, y: (a.y + b.y) / 2 };
      right = Math.max(right, a.x + SELF_LOOP);
    } else if (isBack) {
      const drop = BACK_DROP + backLane++ * BACK_LANE;
      const a = { x: source.x + source.width / 2, y: source.y + source.height };
      const b = { x: target.x + target.width / 2, y: target.y + target.height };
      const floor = Math.max(a.y, b.y) + drop;
      segment = { a, c1: { x: a.x, y: floor }, c2: { x: b.x, y: floor }, b };
      anchor = { x: (a.x + b.x) / 2, y: floor - drop * 0.25 };
      bottom = Math.max(bottom, floor);
    } else {
      const a = { x: source.x + source.width, y: source.y + source.height / 2 + (outPorts.get(edgeIndex) ?? 0) };
      const b = { x: target.x, y: target.y + target.height / 2 + (inPorts.get(edgeIndex) ?? 0) };
      segment = curve(a, b);
      anchor = layer[to] - layer[from] > 1 ? pointAtX(segment, target.x - gap.layer / 2) : pointOn(segment, 0.5);
    }
    const box: Box = { x: anchor.x - labelMain / 2, y: anchor.y - labelCross / 2, width: labelMain, height: labelCross };
    for (let attempt = 0; attempt < placed.length + boxes.length + 1; attempt++) {
      const blocker = [...placed, ...boxes].find((other) => boxesOverlap(box, other, 2));
      if (!blocker) break;
      box.y = blocker.y + blocker.height + 4;
    }
    placed.push(box);
    right = Math.max(right, box.x + box.width);
    bottom = Math.max(bottom, box.y + box.height);
    return { edge, from: keys[from], to: keys[to], path: pathOf(segment), back: isBack, label: { ...toBox(box), lines: label.lines, title: label.title } };
  });

  const nodes: LaidOutNode[] = graph.nodes.map((node, position) => ({ id: keys[position], kind: node.identity.kind, planId: "", iteration: null, manual: false, box: toBox(boxes[position]) }));
  const rootIndex = scenarioRoots[0] ?? (keys.length ? 0 : undefined);
  return {
    orientation,
    nodes,
    byId: new Map(nodes.map((node) => [node.id, node])),
    edges,
    bounds: toBox({ x: 0, y: 0, width: right + PADDING, height: bottom + PADDING }),
    root: rootIndex === undefined ? null : nodes[rootIndex].box,
  };
}

export function layoutAutomationOrientations(graph: AutomationGraph): AutomationLayouts {
  return { horizontal: layoutAutomationGraph(graph, "horizontal"), vertical: layoutAutomationGraph(graph, "vertical") };
}

export interface CanvasSize { width: number; height: number }

function overview(layout: AutomationLayout, size: CanvasSize): ViewportTransform | null {
  return computeFitTransform(layout.bounds, size.width, size.height, { maxScale: 1, fitPadding: FIT_PADDING });
}

/** Keeps left-to-right flow while it reads at full size, otherwise the orientation the measured canvas shows larger. */
export function preferredOrientation(layouts: AutomationLayouts, size: CanvasSize): AutomationOrientation {
  const horizontal = overview(layouts.horizontal, size)?.scale ?? 0;
  const vertical = overview(layouts.vertical, size)?.scale ?? 0;
  return horizontal >= READABLE_SCALE || horizontal >= vertical ? "horizontal" : "vertical";
}

function frameAxis(start: number, extent: number, view: number, focus: number, focusExtent: number, flow: boolean, scale: number): number {
  if (extent * scale <= view - FIT_PADDING * 2) return (view - extent * scale) / 2 - start * scale;
  const wanted = flow ? FIT_PADDING - focus * scale : view / 2 - (focus + focusExtent / 2) * scale;
  return Math.min(FIT_PADDING - start * scale, Math.max(view - FIT_PADDING - (start + extent) * scale, wanted));
}

/** Initial camera: the whole graph when it stays readable, else the root at a readable scale; `whole` forces the overview. */
export function frameAutomation(layout: AutomationLayout, size: CanvasSize, whole: boolean): ViewportTransform | null {
  const fit = overview(layout, size);
  if (!fit || whole || fit.scale >= READABLE_SCALE || !layout.root) return fit;
  const { bounds, root } = layout;
  const horizontal = layout.orientation === "horizontal";
  return {
    scale: READABLE_SCALE,
    x: frameAxis(bounds.x, bounds.width, size.width, root.x, root.width, horizontal, READABLE_SCALE),
    y: frameAxis(bounds.y, bounds.height, size.height, root.y, root.height, !horizontal, READABLE_SCALE),
  };
}
