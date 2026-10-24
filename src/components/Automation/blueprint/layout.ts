import type { Box, Point } from "../../Canvas/layout";
import { computeFitTransform, type ViewportTransform } from "@codeterm/canvas-camera";
import type { BlueprintGraphModel, BlueprintGraphNode, BlueprintGraphWire, BlueprintPin } from "./graphModel";
import { blueprintNodeKinds } from "./kinds";

export const HEADER_HEIGHT = 44;
export const STATUS_HEIGHT = 24;
export const ROW_HEIGHT = 24;
export const BODY_LINE_HEIGHT = 16;
export const BODY_PAD = 10;
const BODY_LINES = { code: 5, prompt: 4, text: 3 } as const;
const BODY_CHAR_WIDTH = 6.4;
const NODE_PAD_BOTTOM = 10;
const COLUMN_GAP = 104;
const ROW_GAP = 32;
export const FRAME_PAD = 20;
export const FRAME_HEADER = 36;
const PADDING = 32;
const LANE_DROP = 22;
const LANE_GAP = 10;
/** Smallest camera scale at which 11px pin labels stay legible. */
export const READABLE_SCALE = 0.85;
export const FIT_PADDING = 28;

export interface PinRow { top: number; height: number; left: BlueprintPin | null; right: BlueprintPin | null }

export interface LaidOutBlueprintNode {
  node: BlueprintGraphNode;
  box: Box;
  bodyHeight: number;
  bodyLines: number;
  statusHeight: number;
  rows: PinRow[];
}

export interface LaidOutWire { wire: BlueprintGraphWire; path: string; from: Point; to: Point }
export interface LaidOutFrame { scope: string; title: string; purpose: string | null; box: Box }

export interface BlueprintLayout {
  nodes: LaidOutBlueprintNode[];
  byKey: Map<string, LaidOutBlueprintNode>;
  wires: LaidOutWire[];
  frames: LaidOutFrame[];
  bounds: Box;
  root: Box | null;
}

function wrapped(text: string, width: number) {
  const perLine = Math.max(8, Math.floor((width - 28) / BODY_CHAR_WIDTH));
  return text.split("\n").reduce((sum, line) => sum + Math.max(1, Math.ceil(line.length / perLine)), 0);
}

export function bodyLines(node: BlueprintGraphNode): number {
  if (!node.body) return 0;
  const width = blueprintNodeKinds[node.type].width;
  const lines = node.body.kind === "code" ? node.body.text.split("\n").length : wrapped(node.body.text, width);
  return Math.min(BODY_LINES[node.body.kind], lines);
}

/** Node geometry the renderer reuses verbatim, so pins sit exactly where wires end. */
export function measureNode(node: BlueprintGraphNode) {
  const lines = bodyLines(node);
  const statusHeight = node.evidence ? STATUS_HEIGHT : 0;
  const bodyHeight = lines ? lines * BODY_LINE_HEIGHT + BODY_PAD * 2 : 0;
  const rows: PinRow[] = [];
  let top = HEADER_HEIGHT + statusHeight + bodyHeight + (lines ? 0 : 4);
  for (let index = 0; index < Math.max(node.inputs.length, node.outputs.length); index++) {
    rows.push({ top, height: ROW_HEIGHT, left: node.inputs[index] ?? null, right: node.outputs[index] ?? null });
    top += ROW_HEIGHT;
  }
  return { width: blueprintNodeKinds[node.type].width, height: top + NODE_PAD_BOTTOM, bodyHeight, bodyLines: lines, statusHeight, rows };
}

function curve(a: Point, b: Point) {
  const reach = Math.max(Math.abs(b.x - a.x) * 0.5, 48);
  return `M ${a.x} ${a.y} C ${a.x + reach} ${a.y}, ${b.x - reach} ${b.y}, ${b.x} ${b.y}`;
}

function detour(a: Point, b: Point, floor: number) {
  return `M ${a.x} ${a.y} C ${a.x + 44} ${a.y}, ${a.x + 44} ${floor}, ${a.x + 88} ${floor} L ${b.x - 88} ${floor} C ${b.x - 44} ${floor}, ${b.x - 44} ${b.y}, ${b.x} ${b.y}`;
}

function loop(a: Point, b: Point, floor: number) {
  const mid = (a.x + b.x) / 2;
  return `M ${a.x} ${a.y} C ${a.x + 72} ${a.y}, ${a.x + 72} ${floor}, ${mid} ${floor} C ${b.x - 72} ${floor}, ${b.x - 72} ${b.y}, ${b.x} ${b.y}`;
}

interface Block { width: number; height: number; positions: Map<string, Point>; frames: { scope: string; box: Box }[] }

/** Ranks flow left to right along forward wires; an expanded callee is one framed block in the rank after its call. */
export function layoutBlueprint(model: BlueprintGraphModel): BlueprintLayout {
  const measured = new Map(model.nodes.map((node) => [node.key, measureNode(node)]));
  const byScope = new Map<string, BlueprintGraphNode[]>();
  for (const node of model.nodes) byScope.set(node.scope, [...(byScope.get(node.scope) ?? []), node]);
  const childOf = new Map(model.scopes.filter((scope) => scope.call).map((scope) => [scope.call!, scope.key]));
  const incoming = new Map<string, BlueprintGraphWire[]>();
  for (const wire of model.wires) if (!wire.back) incoming.set(wire.to.node, [...(incoming.get(wire.to.node) ?? []), wire]);

  const block = (scope: string): Block => {
    const members = byScope.get(scope) ?? [];
    const local = new Set(members.map((node) => node.key));
    const rank = new Map<string, number>();
    const items: { key: string; rank: number; width: number; height: number; child?: Block; scope?: string }[] = [];
    const orphans: string[] = [];
    for (const node of members) {
      const from = (incoming.get(node.key) ?? []).filter((wire) => local.has(wire.from.node));
      if (!from.length && node.control === "result") orphans.push(node.key);
      rank.set(node.key, Math.max(0, ...from.map((wire) => (rank.get(wire.from.node) ?? 0) + 1)));
    }
    const last = Math.max(0, ...rank.values());
    for (const key of orphans) rank.set(key, last);
    for (const node of members) {
      const value = rank.get(node.key)!;
      const size = measured.get(node.key)!;
      items.push({ key: node.key, rank: value, width: size.width, height: size.height });
      const child = childOf.get(node.key);
      if (child) {
        const inner = block(child);
        items.push({ key: child, rank: value + 1, width: inner.width + FRAME_PAD * 2, height: inner.height + FRAME_HEADER + FRAME_PAD * 2, child: inner, scope: child });
      }
    }
    const columns = new Map<number, typeof items>();
    for (const item of items) columns.set(item.rank, [...(columns.get(item.rank) ?? []), item]);
    const positions = new Map<string, Point>();
    const frames: Block["frames"] = [];
    let x = 0, height = 0;
    for (const column of [...columns.keys()].sort((a, b) => a - b).map((key) => columns.get(key)!)) {
      let y = 0;
      for (const item of column) {
        if (item.child) {
          frames.push({ scope: item.scope!, box: { x, y, width: item.width, height: item.height } });
          for (const [key, point] of item.child.positions) positions.set(key, { x: x + FRAME_PAD + point.x, y: y + FRAME_HEADER + FRAME_PAD + point.y });
          for (const frame of item.child.frames) frames.push({ scope: frame.scope, box: { ...frame.box, x: x + FRAME_PAD + frame.box.x, y: y + FRAME_HEADER + FRAME_PAD + frame.box.y } });
        } else positions.set(item.key, { x, y });
        y += item.height + ROW_GAP;
      }
      height = Math.max(height, y - ROW_GAP);
      x += Math.max(...column.map((item) => item.width)) + COLUMN_GAP;
    }
    return { width: Math.max(0, x - COLUMN_GAP), height, positions, frames };
  };

  const root = model.scopes[0] ? block(model.scopes[0].key) : { width: 0, height: 0, positions: new Map<string, Point>(), frames: [] };
  const nodes: LaidOutBlueprintNode[] = model.nodes.flatMap((node) => {
    const point = root.positions.get(node.key);
    if (!point) return [];
    const size = measured.get(node.key)!;
    return [{ node, box: { x: PADDING + point.x, y: PADDING + point.y, width: size.width, height: size.height }, bodyHeight: size.bodyHeight, bodyLines: size.bodyLines, statusHeight: size.statusHeight, rows: size.rows }];
  });
  const scopes = new Map(model.scopes.map((scope) => [scope.key, scope]));
  const frames: LaidOutFrame[] = root.frames.map((frame) => ({ scope: frame.scope, title: scopes.get(frame.scope)?.title ?? "Callee", purpose: scopes.get(frame.scope)?.detail ?? null, box: { ...frame.box, x: PADDING + frame.box.x, y: PADDING + frame.box.y } }));
  const byKey = new Map(nodes.map((entry) => [entry.node.key, entry]));
  const anchor = (key: string, pin: string, side: "in" | "out"): Point | null => {
    const entry = byKey.get(key);
    const row = entry?.rows.find((candidate) => (side === "in" ? candidate.left : candidate.right)?.id === pin);
    if (!entry || !row) return null;
    return { x: entry.box.x + (side === "in" ? 0 : entry.box.width), y: entry.box.y + row.top + ROW_HEIGHT / 2 };
  };

  const contentBottom = Math.max(PADDING, ...nodes.map((entry) => entry.box.y + entry.box.height), ...frames.map((frame) => frame.box.y + frame.box.height));
  let right = Math.max(PADDING, ...nodes.map((entry) => entry.box.x + entry.box.width), ...frames.map((frame) => frame.box.x + frame.box.width));
  let bottom = contentBottom;
  const lanes: { floor: number; from: number; to: number }[] = [];
  const takeLane = (base: number, from: number, to: number) => {
    let floor = base;
    while (lanes.some((lane) => lane.floor === floor && lane.from < to && from < lane.to)) floor += LANE_GAP;
    lanes.push({ floor, from, to });
    return floor;
  };
  let loopLane = 0;
  const wires: LaidOutWire[] = model.wires.flatMap((wire) => {
    const from = anchor(wire.from.node, wire.from.pin, "out");
    const to = anchor(wire.to.node, wire.to.pin, "in");
    if (!from || !to) return [];
    if (wire.back || to.x <= from.x) {
      const floor = contentBottom + LANE_DROP * 2 + loopLane++ * LANE_GAP;
      bottom = Math.max(bottom, floor);
      right = Math.max(right, from.x + 72);
      return [{ wire, from, to, path: loop(from, to, floor) }];
    }
    const top = Math.min(from.y, to.y) - 6, low = Math.max(from.y, to.y) + 6;
    const blockers = nodes.filter((entry) => entry.node.key !== wire.from.node && entry.node.key !== wire.to.node
      && entry.box.x < to.x - 1 && entry.box.x + entry.box.width > from.x + 1);
    if (!blockers.some((entry) => entry.box.y < low && entry.box.y + entry.box.height > top)) return [{ wire, from, to, path: curve(from, to) }];
    const floor = takeLane(Math.max(...blockers.map((entry) => entry.box.y + entry.box.height)) + LANE_DROP, from.x, to.x);
    bottom = Math.max(bottom, floor);
    return [{ wire, from, to, path: detour(from, to, floor) }];
  });
  return { nodes, byKey, wires, frames, bounds: { x: 0, y: 0, width: right + PADDING, height: bottom + PADDING }, root: nodes[0]?.box ?? null };
}

export interface CanvasSize { width: number; height: number }

/** The whole graph while it stays readable, else the focus (or the first node) at a readable scale; `whole` forces the overview. */
export function frameBlueprint(layout: BlueprintLayout, size: CanvasSize, whole: boolean, focusKey: string | null = null): ViewportTransform | null {
  const fit = computeFitTransform(layout.bounds, size.width, size.height, { maxScale: 1, fitPadding: FIT_PADDING });
  const focus = (focusKey ? layout.byKey.get(focusKey)?.box : undefined) ?? layout.root;
  if (!fit || whole || fit.scale >= READABLE_SCALE || !focus) return fit;
  const scale = READABLE_SCALE;
  const { bounds } = layout;
  const fromStart = focus === layout.root;
  const wantX = fromStart ? FIT_PADDING - focus.x * scale : size.width / 2 - (focus.x + focus.width / 2) * scale;
  const wantY = fromStart ? FIT_PADDING - focus.y * scale : size.height / 2 - (focus.y + focus.height / 2) * scale;
  const clamp = (want: number, view: number, start: number, extent: number) => extent * scale <= view - FIT_PADDING * 2
    ? (view - extent * scale) / 2 - start * scale
    : Math.min(FIT_PADDING - start * scale, Math.max(view - FIT_PADDING - (start + extent) * scale, want));
  return { scale, x: clamp(wantX, size.width, bounds.x, bounds.width), y: clamp(wantY, size.height, bounds.y, bounds.height) };
}
