import { bezierPath } from "../Canvas/edges";
import { boxesOverlap, type Box, type LaidOutNode, type Point } from "../Canvas/layout";
import { automationKey, type AutomationEdge, type AutomationGraph } from "./types";

export const NODE_WIDTH = 232;
export const NODE_HEIGHT = 104;
const LAYER_GAP = 208;
const ROW_GAP = 44;
const PADDING = 40;
const CURVE = 0.45;
const PORT_SPREAD = 14;
const LABEL_HEIGHT = 20;
const LABEL_CHAR_WIDTH = 6.4;
const LABEL_MAX_CHARS = 26;
const BACK_DROP = 56;
const BACK_LANE = 22;
const SELF_LOOP = 56;

export interface AutomationEdgeRoute {
  edge: AutomationEdge;
  from: string;
  to: string;
  path: string;
  back: boolean;
  label: Box & { text: string; title: string };
}

export interface AutomationLayout {
  nodes: LaidOutNode[];
  byId: Map<string, LaidOutNode>;
  edges: AutomationEdgeRoute[];
  bounds: Box;
}

function edgeLabel(edge: AutomationEdge) {
  const title = [edge.bucket, edge.handler_type, edge.handler_id, edge.result?.status, edge.evidence.kind].filter(Boolean).join(" · ");
  const full = [edge.bucket, edge.handler_type, edge.result?.status].filter(Boolean).join(" · ");
  const text = full.length > LABEL_MAX_CHARS ? `${full.slice(0, LABEL_MAX_CHARS - 1)}…` : full;
  return { text, title, width: Math.ceil(text.length * LABEL_CHAR_WIDTH) + 14 };
}

function bezierPoint(a: Point, b: Point, t: number): Point {
  const dx = b.x - a.x;
  const h = Math.max(Math.abs(dx) * CURVE, 24);
  const c1 = { x: a.x + (dx >= 0 ? h : -h), y: a.y };
  const c2 = { x: b.x + (dx >= 0 ? -h : h), y: b.y };
  const u = 1 - t;
  return {
    x: u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x,
    y: u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y,
  };
}

function pointAtX(a: Point, b: Point, x: number): Point {
  let best = bezierPoint(a, b, 0.5);
  for (let step = 0; step <= 32; step++) {
    const candidate = bezierPoint(a, b, step / 32);
    if (Math.abs(candidate.x - x) < Math.abs(best.x - x)) best = candidate;
  }
  return best;
}

function portOffsets(count: number): number[] {
  const spread = count > 1 ? Math.min(PORT_SPREAD, (NODE_HEIGHT - 36) / (count - 1)) : 0;
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

export function layoutAutomationGraph(graph: AutomationGraph): AutomationLayout {
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
  const centerY = keys.map(() => 0);
  for (const column of columns) {
    if (!column) continue;
    const desired = new Map(column.map((node) => {
      const above = parents[node].filter((parent) => layer[parent] < layer[node]);
      return [node, above.length ? above.reduce((sum, parent) => sum + centerY[parent], 0) / above.length : Number.NaN];
    }));
    const placedAbove = column.filter((node) => !Number.isNaN(desired.get(node)));
    const ordered = [
      ...placedAbove.sort((a, b) => desired.get(a)! - desired.get(b)! || a - b),
      ...column.filter((node) => Number.isNaN(desired.get(node))),
    ];
    let previous = Number.NEGATIVE_INFINITY;
    for (const node of ordered) {
      const wanted = desired.get(node);
      const y = Math.max(Number.isNaN(wanted!) ? previous + NODE_HEIGHT + ROW_GAP : wanted!, previous + NODE_HEIGHT + ROW_GAP);
      centerY[node] = Number.isFinite(y) ? y : 0;
      previous = centerY[node];
    }
    if (placedAbove.length) {
      const drift = placedAbove.reduce((sum, node) => sum + desired.get(node)! - centerY[node], 0) / placedAbove.length;
      for (const node of ordered) centerY[node] += drift;
    }
  }
  const top = Math.min(...centerY, 0) - NODE_HEIGHT / 2;
  const boxes: Box[] = keys.map((_, position) => ({
    x: PADDING + layer[position] * (NODE_WIDTH + LAYER_GAP),
    y: PADDING + centerY[position] - NODE_HEIGHT / 2 - top,
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
  }));
  const nodes: LaidOutNode[] = graph.nodes.map((node, position) => ({ id: keys[position], kind: node.identity.kind, planId: "", iteration: null, manual: false, box: boxes[position] }));

  const outPorts = new Map<number, number>();
  const inPorts = new Map<number, number>();
  const assignPorts = (side: "from" | "to", ports: Map<number, number>) => {
    keys.forEach((_, node) => {
      const attached = routed.map((edge, edgeIndex) => ({ edge, edgeIndex })).filter(({ edge, edgeIndex }) => edge[side] === node && !back.has(edgeIndex));
      attached.sort((a, b) => boxes[side === "from" ? a.edge.to : a.edge.from].y - boxes[side === "from" ? b.edge.to : b.edge.from].y || a.edgeIndex - b.edgeIndex);
      portOffsets(attached.length).forEach((offset, position) => ports.set(attached[position].edgeIndex, offset));
    });
  };
  assignPorts("from", outPorts);
  assignPorts("to", inPorts);

  const placed: Box[] = [];
  let backLane = 0;
  let bottom = Math.max(...boxes.map((box) => box.y + box.height), PADDING);
  const edges: AutomationEdgeRoute[] = routed.map(({ edge, from, to }, edgeIndex) => {
    const source = boxes[from];
    const target = boxes[to];
    const label = edgeLabel(edge);
    let path: string;
    let anchor: Point;
    const isBack = back.has(edgeIndex) || from === to;
    if (from === to) {
      const a = { x: source.x + source.width, y: source.y + source.height / 2 - 18 };
      const b = { x: a.x, y: a.y + 36 };
      path = `M ${a.x} ${a.y} C ${a.x + SELF_LOOP} ${a.y - 24}, ${b.x + SELF_LOOP} ${b.y + 24}, ${b.x} ${b.y}`;
      anchor = { x: a.x + SELF_LOOP + 4 + label.width / 2, y: (a.y + b.y) / 2 };
    } else if (isBack) {
      const drop = BACK_DROP + backLane++ * BACK_LANE;
      const a = { x: source.x + source.width / 2, y: source.y + source.height };
      const b = { x: target.x + target.width / 2, y: target.y + target.height };
      const floor = Math.max(a.y, b.y) + drop;
      path = `M ${a.x} ${a.y} C ${a.x} ${floor}, ${b.x} ${floor}, ${b.x} ${b.y}`;
      anchor = { x: (a.x + b.x) / 2, y: floor - drop * 0.25 };
    } else {
      const a = { x: source.x + source.width, y: source.y + source.height / 2 + (outPorts.get(edgeIndex) ?? 0) };
      const b = { x: target.x, y: target.y + target.height / 2 + (inPorts.get(edgeIndex) ?? 0) };
      path = bezierPath(a, b, CURVE);
      anchor = layer[to] - layer[from] > 1 ? pointAtX(a, b, target.x - LAYER_GAP / 2) : bezierPoint(a, b, 0.5);
    }
    const box: Box = { x: anchor.x - label.width / 2, y: anchor.y - LABEL_HEIGHT / 2, width: label.width, height: LABEL_HEIGHT };
    for (let attempt = 0; attempt < placed.length + boxes.length + 1; attempt++) {
      const blocker = [...placed, ...boxes].find((other) => boxesOverlap(box, other, 2));
      if (!blocker) break;
      box.y = blocker.y + blocker.height + 4;
    }
    placed.push(box);
    bottom = Math.max(bottom, box.y + box.height, isBack ? anchor.y + LABEL_HEIGHT : 0);
    return { edge, from: keys[from], to: keys[to], path, back: isBack, label: { ...box, text: label.text, title: label.title } };
  });

  const right = Math.max(...boxes.map((box) => box.x + box.width), ...placed.map((box) => box.x + box.width), PADDING);
  return {
    nodes,
    byId: new Map(nodes.map((node) => [node.id, node])),
    edges,
    bounds: { x: 0, y: 0, width: right + PADDING, height: bottom + PADDING },
  };
}
