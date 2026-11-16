import type { DsCanvas, DsCanvasItem, DsCanvasItemKind } from "@codeterm/chat-core";
import type { ViewportBox } from "@codeterm/canvas-camera";

/** Live sandboxed frames one canvas keeps at once; measured in docs/design/ds-canvas-study.md §2.7. */
export const CANVAS_LIVE_FRAME_BUDGET = 4;
export const CANVAS_INLINE_HEIGHT = 320;
export const CANVAS_INLINE_PADDING = 16;
export const CANVAS_DEFAULT_SIZE: Record<DsCanvasItemKind, { w: number; h: number }> = {
  image: { w: 480, h: 320 },
  html: { w: 480, h: 360 },
  page: { w: 640, h: 480 },
  document: { w: 480, h: 400 },
  note: { w: 320, h: 180 },
};
const FLOW_GAP = 40;
const FLOW_ROW_WIDTH = 1600;

export interface PlacedCanvasItem { item: DsCanvasItem; index: number; x: number; y: number; w: number; h: number }
export interface CanvasLayout { items: PlacedCanvasItem[]; bounds: ViewportBox }
/** `key` names the view or item a target stands for, so the pager can follow it across reloads. */
export interface CanvasPagerTarget { key: string; label: string; bounds: ViewportBox }
export interface CanvasPagerPosition { key: string; index: number }

export const isLiveFrameItem = (item: DsCanvasItem) => item.kind === "html" || item.kind === "page";
const NOTE_LABEL_CHARS = 40;

/** A note is named by its first line of text, without Markdown marks. */
export function noteLabel(markdown: string): string {
  const line = markdown.split("\n").map((text) => text.replace(/^\s*(?:#{1,6}\s+|[-*+>]\s+|\d+[.)]\s+)*/, "").replace(/[*_`~]/g, "").trim()).find(Boolean);
  if (!line) return "Note";
  return line.length > NOTE_LABEL_CHARS ? `${line.slice(0, NOTE_LABEL_CHARS - 1).trimEnd()}…` : line;
}

export const canvasItemLabel = (item: DsCanvasItem) => item.title || ("src" in item ? item.src.split(/[\\/]/).pop() || item.src : item.kind === "html" ? "HTML" : noteLabel(item.markdown));

function union(boxes: ViewportBox[]): ViewportBox {
  const left = Math.min(...boxes.map((box) => box.x)), top = Math.min(...boxes.map((box) => box.y));
  const right = Math.max(...boxes.map((box) => box.x + box.width)), bottom = Math.max(...boxes.map((box) => box.y + box.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}
export const placedBox = (placed: PlacedCanvasItem): ViewportBox => ({ x: placed.x, y: placed.y, width: placed.w, height: placed.h });

/** Items with both x and y keep them; the rest flow in wrapped rows below the placed ones. */
export function layoutCanvas(canvas: DsCanvas): CanvasLayout {
  const sized = canvas.items.map((item, index) => ({ item, index, w: item.w ?? CANVAS_DEFAULT_SIZE[item.kind].w, h: item.h ?? CANVAS_DEFAULT_SIZE[item.kind].h }));
  const fixed = sized.filter(({ item }) => item.x !== undefined && item.y !== undefined).map((entry) => ({ ...entry, x: entry.item.x!, y: entry.item.y! }));
  const fixedBounds = fixed.length ? union(fixed.map(placedBox)) : null;
  const flowLeft = fixedBounds?.x ?? 0;
  let x = flowLeft, y = fixedBounds ? fixedBounds.y + fixedBounds.height + FLOW_GAP : 0, rowHeight = 0;
  const flowed = sized.filter(({ item }) => item.x === undefined || item.y === undefined).map((entry) => {
    if (x > flowLeft && x - flowLeft + entry.w > FLOW_ROW_WIDTH) { x = flowLeft; y += rowHeight + FLOW_GAP; rowHeight = 0; }
    const placed = { ...entry, x, y };
    x += entry.w + FLOW_GAP;
    rowHeight = Math.max(rowHeight, entry.h);
    return placed;
  });
  const items = [...fixed, ...flowed].sort((a, b) => a.index - b.index);
  return { items, bounds: union(items.map(placedBox)) };
}

/** A view name such as `side_by-side` reads as "Side by side"; written titles keep their case. */
export function sentenceCase(name: string): string {
  const words = name.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim() || name;
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The pager steps through named views, or through items when a canvas names none. */
export function canvasPagerTargets(canvas: DsCanvas, layout: CanvasLayout): CanvasPagerTarget[] {
  if (canvas.views.length) return canvas.views.map((view) => ({ key: `view:${view.name}`, label: sentenceCase(view.name), bounds: { x: view.x, y: view.y, width: view.w, height: view.h } }));
  return layout.items.map((placed) => ({ key: `item:${placed.item.id}`, label: canvasItemLabel(placed.item), bounds: placedBox(placed) }));
}

/** The pager's target after the targets changed: the same view or item while it exists, else the nearest index. */
export function reconcilePager(targets: readonly CanvasPagerTarget[], position: CanvasPagerPosition | null): number | null {
  if (position === null || targets.length === 0) return null;
  const same = targets.findIndex((target) => target.key === position.key);
  return same >= 0 ? same : Math.min(position.index, targets.length - 1);
}

/** `null` is the whole canvas; stepping past either end returns to it. */
export function stepCanvasPager(active: number | null, count: number, direction: number): number | null {
  if (count <= 0) return null;
  if (active === null) return direction > 0 ? 0 : count - 1;
  const next = active + Math.sign(direction);
  return next < 0 || next >= count ? null : next;
}

export function canvasPagerLabel(active: number | null, count: number): string {
  return active === null ? "All" : `${active + 1} / ${count}`;
}

/** The selected frame stays live first, then visible frames in item order, up to the budget. */
export function chooseLiveFrames(frames: readonly { id: string; visible: boolean }[], selected: string | null, budget = CANVAS_LIVE_FRAME_BUDGET): Set<string> {
  const live = new Set<string>();
  if (selected !== null && frames.some((frame) => frame.id === selected)) live.add(selected);
  for (const frame of frames) {
    if (live.size >= budget) break;
    if (frame.visible) live.add(frame.id);
  }
  return live;
}

/** Scale and offset that fit the whole canvas, centred, inside the padding of a static preview box. */
export function fitPreview(bounds: ViewportBox, width: number, height: number, padding = CANVAS_INLINE_PADDING): { scale: number; x: number; y: number } {
  const innerWidth = width - 2 * padding, innerHeight = height - 2 * padding;
  if (innerWidth <= 0 || innerHeight <= 0) return { scale: 0, x: 0, y: 0 };
  const scale = Math.min(innerWidth / bounds.width, innerHeight / bounds.height, 1);
  return { scale, x: (width - bounds.width * scale) / 2 - bounds.x * scale, y: (height - bounds.height * scale) / 2 - bounds.y * scale };
}

/** The pager index and rectangle of a named view, or `null` when the canvas names no such view. */
export function canvasViewTarget(canvas: DsCanvas, name: string | undefined): { index: number; bounds: ViewportBox } | null {
  const index = name === undefined ? -1 : canvas.views.findIndex((view) => view.name === name);
  if (index < 0) return null;
  const view = canvas.views[index];
  return { index, bounds: { x: view.x, y: view.y, width: view.w, height: view.h } };
}

export const CANVAS_MIN_ITEM = { w: 120, h: 80 };
export const CANVAS_DRAG_THRESHOLD = 4;
export const CANVAS_NUDGE = { step: 8, fine: 1 };
export const CANVAS_NOTE_SIZE = { w: 320, h: 180 };
const NOTE_STACK_OFFSET = 24;

/** A bottom-right resize by a world delta; images keep their aspect ratio, everything keeps the minimum size. */
export function resizeFrame(start: { w: number; h: number }, dx: number, dy: number, keepAspect: boolean): { w: number; h: number } {
  const w = Math.max(CANVAS_MIN_ITEM.w, start.w + dx);
  const h = Math.max(CANVAS_MIN_ITEM.h, start.h + dy);
  if (!keepAspect || start.w <= 0 || start.h <= 0) return { w: Math.round(w), h: Math.round(h) };
  const ratio = start.w / start.h;
  let width = Math.abs(dx) >= Math.abs(dy) ? w : h * ratio;
  if (width / ratio < CANVAS_MIN_ITEM.h) width = CANVAS_MIN_ITEM.h * ratio;
  width = Math.max(width, CANVAS_MIN_ITEM.w);
  return { w: Math.round(width), h: Math.round(width / ratio) };
}

/** Where a new note goes: centred on the view, stepped by 24px while a note already sits exactly there. */
export function noteSpot(center: { x: number; y: number }, placed: readonly PlacedCanvasItem[]): { x: number; y: number } {
  let x = Math.round(center.x - CANVAS_NOTE_SIZE.w / 2), y = Math.round(center.y - CANVAS_NOTE_SIZE.h / 2);
  while (placed.some((entry) => entry.item.kind === "note" && entry.x === x && entry.y === y)) { x += NOTE_STACK_OFFSET; y += NOTE_STACK_OFFSET; }
  return { x, y };
}
