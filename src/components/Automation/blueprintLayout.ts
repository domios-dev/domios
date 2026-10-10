import type { Box, Point } from "../Canvas/layout";
import { computeFitTransform, type ViewportTransform } from "../Canvas/useCanvasViewport";
import type { BlueprintModel, BlueprintNode, BlueprintPin, BlueprintWire } from "./blueprintModel";

import { blueprintNodeKinds } from "./blueprintKinds";
export const HEADER_HEIGHT = 42;
export const ROW_HEIGHT = 24;
export const BODY_LINE_HEIGHT = 16;
export const BODY_PAD = 10;
export const PROMPT_CAPTION_HEIGHT = 18;
const BODY_LINES = { code: 6, prompt: 5, text: 4 } as const;
const BODY_CHAR_WIDTH = 6.4;
const NODE_PAD_BOTTOM = 8;
const COLUMN_GAP = 76;
const ACTION_GAP = 14;
const STAGE_GAP = 56;
const DEPTH_GAP = 112;
export const FRAME_PAD = 20;
export const FRAME_HEADER = 40;
const PADDING = 24;
const LOOP_DROP = 48;
const LOOP_LANE = 18;
/** Smallest camera scale at which 11px pin labels stay legible. */
export const READABLE_SCALE = 0.85;
export const FIT_PADDING = 28;

export interface PinRow { top: number; height: number; left: BlueprintPin | null; right: BlueprintPin | null }

export interface LaidOutBlueprintNode {
  node: BlueprintNode;
  box: Box;
  bodyHeight: number;
  bodyLines: number;
  rows: PinRow[];
}

export interface LaidOutWire { wire: BlueprintWire; path: string; from: Point; to: Point }
export interface LaidOutFrame { stageKey: string; title: string; purpose: string | null; box: Box }

export interface BlueprintLayout {
  nodes: LaidOutBlueprintNode[];
  byKey: Map<string, LaidOutBlueprintNode>;
  wires: LaidOutWire[];
  frames: LaidOutFrame[];
  bounds: Box;
  root: Box | null;
}

export function pinKey(node: string, pin: string) {
  return `${node}#${pin}`;
}

function wrapped(text: string, width: number) {
  const perLine = Math.max(8, Math.floor((width - 24) / BODY_CHAR_WIDTH));
  return text.split("\n").reduce((sum, line) => sum + Math.max(1, Math.ceil(line.length / perLine)), 0);
}

export function bodyLines(node: BlueprintNode): number {
  if (!node.body) return 0;
  const width = blueprintNodeKinds[node.type].width;
  const lines = node.body.kind === "code" ? node.body.text.split("\n").length : wrapped(node.body.text, width);
  return Math.min(BODY_LINES[node.body.kind], lines);
}

/** Node geometry the renderer reuses verbatim, so pins sit exactly where wires end. */
export function measureNode(node: BlueprintNode) {
  const lines = bodyLines(node);
  const bodyHeight = lines ? lines * BODY_LINE_HEIGHT + BODY_PAD * 2 + (node.body?.kind === "prompt" ? PROMPT_CAPTION_HEIGHT : 0) : 0;
  const rows: PinRow[] = [];
  let top = HEADER_HEIGHT + bodyHeight;
  for (let index = 0; index < Math.max(node.inputs.length, node.outputs.length); index++) {
    const left = node.inputs[index];
    const right = node.outputs[index];
    const height = ROW_HEIGHT;
    rows.push({ top, height, left: left ?? null, right: right ?? null });
    top += height;
  }
  return { width: blueprintNodeKinds[node.type].width, height: top + NODE_PAD_BOTTOM, bodyHeight, bodyLines: lines, rows };
}

function curve(a: Point, b: Point) {
  const reach = Math.max(Math.abs(b.x - a.x) * 0.5, 48);
  return `M ${a.x} ${a.y} C ${a.x + reach} ${a.y}, ${b.x - reach} ${b.y}, ${b.x} ${b.y}`;
}

function loop(a: Point, b: Point, floor: number) {
  const mid = (a.x + b.x) / 2;
  return `M ${a.x} ${a.y} C ${a.x + 72} ${a.y}, ${a.x + 72} ${floor}, ${mid} ${floor} C ${b.x - 72} ${floor}, ${b.x - 72} ${b.y}, ${b.x} ${b.y}`;
}

/** Stages flow left to right by activation depth; inside a stage Trigger, Source, decision and actions take one column each. */
export function layoutBlueprint(model: BlueprintModel): BlueprintLayout {
  const measured = new Map(model.nodes.map((node) => [node.key, measureNode(node)]));
  const byNode = new Map(model.nodes.map((node) => [node.key, node]));
  const blocks = model.stages.map((stage) => {
    const local = new Map<string, Point>();
    const [trigger, source, decision, ...actions] = stage.nodes;
    let x = 0;
    for (const key of [trigger, source, decision]) {
      local.set(key, { x, y: 0 });
      x += measured.get(key)!.width + COLUMN_GAP;
    }
    let y = 0;
    for (const key of actions) {
      local.set(key, { x, y });
      y += measured.get(key)!.height + ACTION_GAP;
    }
    const keys = [trigger, source, decision, ...actions];
    const width = Math.max(...keys.map((key) => local.get(key)!.x + measured.get(key)!.width));
    const height = Math.max(...keys.map((key) => local.get(key)!.y + measured.get(key)!.height));
    return { stage, local, width: width + FRAME_PAD * 2, height: height + FRAME_HEADER + FRAME_PAD * 2 };
  });

  const depths = [...new Set(model.stages.map((stage) => stage.depth))].sort((a, b) => a - b);
  const columnX = new Map<number, number>();
  let cursor = PADDING;
  for (const depth of depths) {
    columnX.set(depth, cursor);
    cursor += Math.max(...blocks.filter((block) => block.stage.depth === depth).map((block) => block.width)) + DEPTH_GAP;
  }
  const columnY = new Map<number, number>();
  const nodes: LaidOutBlueprintNode[] = [];
  const frames: LaidOutFrame[] = [];
  for (const block of blocks) {
    const frameX = columnX.get(block.stage.depth)!;
    const frameY = columnY.get(block.stage.depth) ?? PADDING;
    columnY.set(block.stage.depth, frameY + block.height + STAGE_GAP);
    frames.push({ stageKey: block.stage.key, title: block.stage.title, purpose: block.stage.purpose, box: { x: frameX, y: frameY, width: block.width, height: block.height } });
    for (const [key, point] of block.local) {
      const size = measured.get(key)!;
      nodes.push({ node: byNode.get(key)!, box: { x: frameX + FRAME_PAD + point.x, y: frameY + FRAME_HEADER + FRAME_PAD + point.y, width: size.width, height: size.height }, bodyHeight: size.bodyHeight, bodyLines: size.bodyLines, rows: size.rows });
    }
  }
  const byKey = new Map(nodes.map((entry) => [entry.node.key, entry]));
  const anchor = (key: string, pin: string, side: "in" | "out"): Point | null => {
    const entry = byKey.get(key);
    const row = entry?.rows.find((candidate) => (side === "in" ? candidate.left : candidate.right)?.id === pin);
    if (!entry || !row) return null;
    return { x: entry.box.x + (side === "in" ? 0 : entry.box.width), y: entry.box.y + row.top + ROW_HEIGHT / 2 };
  };

  let right = Math.max(PADDING, ...frames.map((frame) => frame.box.x + frame.box.width));
  let bottom = Math.max(PADDING, ...frames.map((frame) => frame.box.y + frame.box.height));
  const contentBottom = bottom;
  let lane = 0;
  const wires: LaidOutWire[] = model.wires.flatMap((wire) => {
    const from = anchor(wire.from.node, wire.from.pin, "out");
    const to = anchor(wire.to.node, wire.to.pin, "in");
    if (!from || !to) return [];
    if (!wire.back) return [{ wire, from, to, path: curve(from, to) }];
    const floor = contentBottom + LOOP_DROP + lane++ * LOOP_LANE;
    bottom = Math.max(bottom, floor);
    right = Math.max(right, from.x + 72);
    return [{ wire, from, to, path: loop(from, to, floor) }];
  });
  const rootFrame = frames[0]?.box ?? null;
  return { nodes, byKey, wires, frames, bounds: { x: 0, y: 0, width: right + PADDING, height: bottom + PADDING }, root: rootFrame };
}

export interface CanvasSize { width: number; height: number }

/** The whole graph while it stays readable, else the focus (or the first stage) at a readable scale; `whole` forces the overview. */
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
