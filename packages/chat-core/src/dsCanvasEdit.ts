import { DS_CANVAS_ITEM_LIMIT, parseDsCanvas, type DsCanvas, type DsCanvasItem } from "./dsCanvas";

/**
 * One owner edit of a canvas file. `frame` sets x, y, w or h, and `null` returns that field to the
 * flow layout. Edits address items by id, so they still land when an agent rewrote other items.
 */
export type DsCanvasEdit =
  | { kind: "frame"; id: string; frame: Partial<Record<"x" | "y" | "w" | "h", number | null>> }
  | { kind: "text"; id: string; markdown: string }
  | { kind: "delete"; id: string }
  | { kind: "insert"; index: number; item: Record<string, unknown> };

type RawItem = Record<string, unknown>;
type RawCanvas = Record<string, unknown> & { items: RawItem[] };

function readRaw(text: string): RawCanvas | null {
  if (!parseDsCanvas(text).ok) return null;
  return JSON.parse(text) as RawCanvas;
}

/** Items without an id get the id the canvas shows for them, so later edits keep their target. */
function pinIds(canvas: RawCanvas): RawCanvas {
  return { ...canvas, items: canvas.items.map((item, index) => typeof item.id === "string" ? item : { id: `item-${index + 1}`, ...item }) };
}

const indexOf = (canvas: RawCanvas, id: string) => canvas.items.findIndex((item) => item.id === id);
const serialize = (canvas: RawCanvas) => `${JSON.stringify(canvas, null, 2)}\n`;

function applyRaw(canvas: RawCanvas, edit: DsCanvasEdit): RawCanvas | null {
  if (edit.kind === "insert") {
    if (canvas.items.length >= DS_CANVAS_ITEM_LIMIT || typeof edit.item.id !== "string" || indexOf(canvas, edit.item.id) >= 0) return null;
    const items = [...canvas.items];
    items.splice(Math.max(0, Math.min(edit.index, items.length)), 0, edit.item);
    return { ...canvas, items };
  }
  const index = indexOf(canvas, edit.id);
  if (index < 0) return null;
  const items = [...canvas.items];
  if (edit.kind === "delete") {
    if (items.length <= 1) return null;
    items.splice(index, 1);
    return { ...canvas, items };
  }
  const item = { ...items[index] };
  if (edit.kind === "text") {
    if (item.kind !== "note") return null;
    item.markdown = edit.markdown;
  } else {
    for (const [field, value] of Object.entries(edit.frame)) {
      if (value === null) delete item[field];
      else if (value !== undefined) item[field] = Math.round(value);
    }
  }
  items[index] = item;
  return { ...canvas, items };
}

/** Applies an edit to the file text; `null` when it no longer applies or would make the canvas invalid. */
export function applyDsCanvasEdit(text: string, edit: DsCanvasEdit): string | null {
  const raw = readRaw(text);
  const next = raw && applyRaw(pinIds(raw), edit);
  if (!next) return null;
  const serialized = serialize(next);
  return parseDsCanvas(serialized).ok ? serialized : null;
}

/** The edit that undoes `edit` on `text`, read before it applies. */
export function invertDsCanvasEdit(text: string, edit: DsCanvasEdit): DsCanvasEdit | null {
  const raw = readRaw(text);
  if (!raw) return null;
  const canvas = pinIds(raw);
  if (edit.kind === "insert") return typeof edit.item.id === "string" ? { kind: "delete", id: edit.item.id } : null;
  const index = indexOf(canvas, edit.id);
  if (index < 0) return null;
  const item = canvas.items[index];
  if (edit.kind === "delete") return { kind: "insert", index, item };
  if (edit.kind === "text") return typeof item.markdown === "string" ? { kind: "text", id: edit.id, markdown: item.markdown } : null;
  const frame: Partial<Record<"x" | "y" | "w" | "h", number | null>> = {};
  for (const field of Object.keys(edit.frame) as ("x" | "y" | "w" | "h")[]) frame[field] = typeof item[field] === "number" ? item[field] as number : null;
  return { kind: "frame", id: edit.id, frame };
}

/** A note id the canvas does not use yet. */
export function nextDsCanvasNoteId(canvas: DsCanvas): string {
  const used = new Set(canvas.items.map((item) => item.id));
  let index = 1;
  while (used.has(`note-${index}`)) index++;
  return `note-${index}`;
}

const sameItem = (a: DsCanvasItem, b: DsCanvasItem) => JSON.stringify(a) === JSON.stringify(b);

/**
 * Keeps the previous item objects wherever an item did not change, so a reload or an edit
 * re-renders only the items it touched and the board updates in place.
 */
export function reconcileDsCanvas(previous: DsCanvas | null, next: DsCanvas): DsCanvas {
  if (!previous) return next;
  const byId = new Map(previous.items.map((item) => [item.id, item]));
  const items = next.items.map((item) => {
    const prior = byId.get(item.id);
    return prior && sameItem(prior, item) ? prior : item;
  });
  const views = JSON.stringify(previous.views) === JSON.stringify(next.views) ? previous.views : next.views;
  const unchanged = previous.title === next.title && views === previous.views && items.length === previous.items.length && items.every((item, index) => item === previous.items[index]);
  return unchanged ? previous : { ...(next.title !== undefined ? { title: next.title } : {}), items, views };
}
