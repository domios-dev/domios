import { isDsAbsolutePath } from "./dsBlocks.schema";

const DS_CANVAS_BODY_LIMIT = 200_000;
export const DS_CANVAS_ITEM_LIMIT = 64;
const DS_CANVAS_FILE_LIMIT = 32;
const DS_CANVAS_VIEW_LIMIT = 16;
const POSITION_LIMIT = 1_000_000;
const SIZE_LIMIT = 100_000;
const VIEW_NAME_BYTES = 80;
// Native JSON readers refuse unpaired surrogates; refusing them here keeps both guards identical.
const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;

export const DS_CANVAS_ITEM_KINDS = ["image", "html", "page", "document", "note"] as const;
export type DsCanvasItemKind = typeof DS_CANVAS_ITEM_KINDS[number];
type DsCanvasFileKind = Extract<DsCanvasItemKind, "image" | "page" | "document">;

interface DsCanvasItemFrame { id: string; x?: number; y?: number; w?: number; h?: number; title?: string }
export type DsCanvasItem =
  | DsCanvasItemFrame & { kind: DsCanvasFileKind; src: string }
  | DsCanvasItemFrame & { kind: "html"; html: string }
  | DsCanvasItemFrame & { kind: "note"; markdown: string };
export interface DsCanvasView { name: string; x: number; y: number; w: number; h: number }
export interface DsCanvas { title?: string; items: DsCanvasItem[]; views: DsCanvasView[] }
export type DsCanvasParse = { ok: true; canvas: DsCanvas } | { ok: false; reason: string };

const CONTENT_FIELD: Record<DsCanvasItemKind, "src" | "html" | "markdown"> = { image: "src", page: "src", document: "src", html: "html", note: "markdown" };
const FRAME_FIELDS = ["id", "kind", "x", "y", "w", "h", "title"];
const bytes = (value: string) => new TextEncoder().encode(value).length;
const isRecord = (value: unknown): value is Record<string, unknown> => Boolean(value) && typeof value === "object" && !Array.isArray(value);
const isKind = (value: unknown): value is DsCanvasItemKind => typeof value === "string" && (DS_CANVAS_ITEM_KINDS as readonly string[]).includes(value);
const position = (value: unknown) => typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= POSITION_LIMIT;
const extent = (value: unknown) => typeof value === "number" && Number.isFinite(value) && value > 0 && value <= SIZE_LIMIT;
const unknownField = (record: Record<string, unknown>, allowed: readonly string[]) => Object.keys(record).find((key) => !allowed.includes(key));

class CanvasError extends Error {}
function fail(reason: string): never { throw new CanvasError(reason); }

function readItem(value: unknown, index: number, ids: Set<string>): DsCanvasItem {
  const label = `Item ${index + 1}`;
  if (!isRecord(value)) fail(`${label} is not an object.`);
  if (!isKind(value.kind)) fail(`${label} needs a kind: ${DS_CANVAS_ITEM_KINDS.join(", ")}.`);
  const kind = value.kind;
  const content = CONTENT_FIELD[kind];
  const extra = unknownField(value, [...FRAME_FIELDS, content]);
  if (extra !== undefined) fail(`${label} has an unknown field "${extra}".`);
  if (value.id !== undefined && (typeof value.id !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(value.id))) fail(`${label} has an id that is not 1 to 64 letters, digits, "_" or "-".`);
  const id = typeof value.id === "string" ? value.id : `item-${index + 1}`;
  if (ids.has(id)) fail(`${label} repeats the id "${id}".`);
  ids.add(id);
  for (const field of ["x", "y"] as const) if (value[field] !== undefined && !position(value[field])) fail(`${label} has an invalid ${field}.`);
  for (const field of ["w", "h"] as const) if (value[field] !== undefined && !extent(value[field])) fail(`${label} has an invalid ${field}; sizes are positive numbers.`);
  if (value.title !== undefined && typeof value.title !== "string") fail(`${label} has a title that is not text.`);
  const body = value[content];
  if (typeof body !== "string") fail(`${label} (${kind}) needs "${content}" text.`);
  if (content === "src" && !isDsAbsolutePath(body)) fail(`${label} needs an absolute file path in "src".`);
  const frame: DsCanvasItemFrame = { id };
  for (const field of ["x", "y", "w", "h"] as const) if (value[field] !== undefined) frame[field] = value[field] as number;
  if (value.title !== undefined) frame.title = value.title as string;
  if (kind === "html") return { ...frame, kind, html: body };
  if (kind === "note") return { ...frame, kind, markdown: body };
  return { ...frame, kind, src: body };
}

function readView(value: unknown, index: number, names: Set<string>): DsCanvasView {
  const label = `View ${index + 1}`;
  if (!isRecord(value)) fail(`${label} is not an object.`);
  const extra = unknownField(value, ["name", "x", "y", "w", "h"]);
  if (extra !== undefined) fail(`${label} has an unknown field "${extra}".`);
  if (typeof value.name !== "string" || !value.name || bytes(value.name) > VIEW_NAME_BYTES) fail(`${label} needs a name of at most ${VIEW_NAME_BYTES} bytes.`);
  if (names.has(value.name)) fail(`${label} repeats the name "${value.name}".`);
  names.add(value.name);
  if (!position(value.x) || !position(value.y) || !extent(value.w) || !extent(value.h)) fail(`${label} needs x, y and positive w and h.`);
  return { name: value.name, x: value.x as number, y: value.y as number, w: value.w as number, h: value.h as number };
}

function readJson(text: string): { ok: true; document: unknown } | { ok: false; reason: string } {
  if (bytes(text) > DS_CANVAS_BODY_LIMIT) return { ok: false, reason: `The canvas is larger than ${DS_CANVAS_BODY_LIMIT} bytes.` };
  try {
    return { ok: true, document: JSON.parse(text, (key, value: unknown) => {
      if (LONE_SURROGATE.test(key) || typeof value === "string" && LONE_SURROGATE.test(value)) throw new SyntaxError("Text contains an unpaired surrogate escape");
      return value;
    }) };
  } catch (error) { return { ok: false, reason: `The JSON is invalid: ${error instanceof Error ? error.message : String(error)}` }; }
}

/** Reads a `<ds-canvas>` body; the reason is one sentence for the block's Details. */
export function parseDsCanvas(body: string): DsCanvasParse {
  const json = readJson(body);
  return json.ok ? readCanvas(json.document) : json;
}

/** A canvas file is named `NAME.canvas.json`; a bare `.canvas` belongs to another format. */
export const isDsCanvasFilePath = (path: string) => /[^\\/]\.canvas\.json$/i.test(path);

/** The header that opens every canvas file; the major version lets the format change later. */
export const DS_CANVAS_FILE_FORMAT = "domios.canvas/1";
const FILE_FORMAT = /^domios\.canvas\/(\d+)(?:\.\d+)?$/;

/** Reads a `.canvas.json` file: `"format": "domios.canvas/1"` first, then exactly an inline body's fields. */
export function parseDsCanvasFile(text: string): DsCanvasParse {
  const json = readJson(text);
  if (!json.ok) return json;
  const document = json.document;
  if (!isRecord(document) || Object.keys(document)[0] !== "format" || typeof document.format !== "string") {
    return { ok: false, reason: `A canvas file starts with "format": "${DS_CANVAS_FILE_FORMAT}" as its first field.` };
  }
  const major = FILE_FORMAT.exec(document.format)?.[1];
  if (major !== "1") return { ok: false, reason: `This canvas file uses the format "${document.format}", which this version cannot read.` };
  const body: Record<string, unknown> = { ...document };
  delete body.format;
  return readCanvas(body);
}

function readCanvas(document: unknown): DsCanvasParse {
  try {
    if (!isRecord(document)) fail("The canvas must be a JSON object.");
    const extra = unknownField(document, ["title", "items", "views"]);
    if (extra !== undefined) fail(`The canvas has an unknown field "${extra}".`);
    if (document.title !== undefined && typeof document.title !== "string") fail("The canvas title is not text.");
    if (!Array.isArray(document.items) || document.items.length < 1 || document.items.length > DS_CANVAS_ITEM_LIMIT) fail(`The canvas needs 1 to ${DS_CANVAS_ITEM_LIMIT} items.`);
    if (document.views !== undefined && (!Array.isArray(document.views) || document.views.length > DS_CANVAS_VIEW_LIMIT)) fail(`Views must be a list of at most ${DS_CANVAS_VIEW_LIMIT}.`);
    const ids = new Set<string>();
    const items = document.items.map((item, index) => readItem(item, index, ids));
    if (items.filter((item) => CONTENT_FIELD[item.kind] === "src").length > DS_CANVAS_FILE_LIMIT) fail(`The canvas may show at most ${DS_CANVAS_FILE_LIMIT} files.`);
    const names = new Set<string>();
    const views = ((document.views ?? []) as unknown[]).map((view, index) => readView(view, index, names));
    return { ok: true, canvas: { ...(document.title !== undefined ? { title: document.title as string } : {}), items, views } };
  } catch (error) {
    if (error instanceof CanvasError) return { ok: false, reason: error.message };
    throw error;
  }
}

/** The local files a canvas previews, in item order; the preview guard admits exactly these. */
export function dsCanvasFilePaths(body: string): string[] {
  const parsed = parseDsCanvas(body);
  return parsed.ok ? parsed.canvas.items.flatMap((item) => "src" in item ? [item.src] : []) : [];
}
