export interface Size {
  width: number;
  height: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ClientRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface FrameMetadata {
  deviceWidth: number;
  deviceHeight: number;
  offsetTop?: number;
  pageScaleFactor?: number;
  scrollOffsetX?: number;
  scrollOffsetY?: number;
  timestamp?: number;
}

export interface StreamViewport {
  w: number;
  h: number;
  dpr: number;
}

export interface PagePoint {
  x: number;
  y: number;
}

export interface StreamLayout {
  backing: Size;
  draw: Rect;
  drawCss: Rect;
}

export type StreamInput =
  | { kind: "click"; x: number; y: number; button: "left"; clickCount: number }
  | { kind: "wheel"; x: number; y: number; deltaX: number; deltaY: number }
  | { kind: "text"; text: string }
  | { kind: "key"; key: string; code: string; windowsVirtualKeyCode: number; modifiers: number; text?: string };

export interface ModifierState {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

export interface KeyLike extends ModifierState {
  key: string;
  code: string;
  isComposing?: boolean;
}

export type KeyOrigin = "surface" | "editable";

export interface KeyContext {
  interactive: boolean;
  picking: boolean;
  origin: KeyOrigin;
}

export type KeyDecision =
  | { action: "forward"; input: StreamInput }
  | { action: "exit-interaction" }
  | { action: "cancel-pick" }
  | { action: "keep" };

export const MODIFIER = { alt: 1, ctrl: 2, meta: 4, shift: 8 } as const;

export const VIRTUAL_KEY_CODES: Readonly<Record<string, number>> = {
  Backspace: 8,
  Tab: 9,
  Enter: 13,
  NumpadEnter: 13,
  Escape: 27,
  PageUp: 33,
  PageDown: 34,
  End: 35,
  Home: 36,
  ArrowLeft: 37,
  ArrowUp: 38,
  ArrowRight: 39,
  ArrowDown: 40,
  Delete: 46,
};

const WHEEL_LINE_PX = 16;
const MAX_DPR = 4;

export function sanitizeDpr(dpr: number): number {
  if (!Number.isFinite(dpr) || dpr <= 0) return 1;
  return Math.min(dpr, MAX_DPR);
}

export function backingSize(css: Size, dpr: number): Size {
  const ratio = sanitizeDpr(dpr);
  return { width: Math.max(0, Math.round(css.width * ratio)), height: Math.max(0, Math.round(css.height * ratio)) };
}

export function letterboxRect(frame: Size, canvas: Size): Rect {
  if (frame.width <= 0 || frame.height <= 0 || canvas.width <= 0 || canvas.height <= 0) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }
  const scale = Math.min(canvas.width / frame.width, canvas.height / frame.height);
  const width = frame.width * scale;
  const height = frame.height * scale;
  return { x: (canvas.width - width) / 2, y: (canvas.height - height) / 2, width, height };
}

export function layoutStream(frame: Size, css: Size, dpr: number): StreamLayout {
  const backing = backingSize(css, dpr);
  const draw = letterboxRect(frame, backing);
  const kx = css.width > 0 ? backing.width / css.width : 1;
  const ky = css.height > 0 ? backing.height / css.height : 1;
  const drawCss = { x: draw.x / kx, y: draw.y / ky, width: draw.width / kx, height: draw.height / ky };
  return { backing, draw, drawCss };
}

export function canvasToPage(
  clientX: number,
  clientY: number,
  canvasRect: ClientRect,
  drawRect: Rect,
  frameMeta: FrameMetadata | null,
  layoutSize: Size = canvasRect,
): PagePoint | null {
  if (drawRect.width <= 0 || drawRect.height <= 0 || !Number.isFinite(clientX) || !Number.isFinite(clientY)) return null;
  if (canvasRect.width <= 0 || canvasRect.height <= 0) return null;
  const localX = (clientX - canvasRect.left) * layoutSize.width / canvasRect.width - drawRect.x;
  const localY = (clientY - canvasRect.top) * layoutSize.height / canvasRect.height - drawRect.y;
  if (localX < 0 || localY < 0 || localX > drawRect.width || localY > drawRect.height) return null;
  const deviceWidth = frameMeta?.deviceWidth || drawRect.width;
  const deviceHeight = frameMeta?.deviceHeight || drawRect.height;
  const scale = frameMeta?.pageScaleFactor || 1;
  const offsetTop = frameMeta?.offsetTop ?? 0;
  const deviceX = (localX / drawRect.width) * deviceWidth;
  const deviceY = (localY / drawRect.height) * deviceHeight;
  return { x: deviceX / scale, y: (deviceY - offsetTop) / scale };
}

/** A page area in document CSS px: viewport position plus the frame's scroll offset. */
export interface PageRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);

/** Stage-local point pinned inside the drawn frame, so a drag past the letterbox stops at its edge. */
export function clampToDraw(point: { x: number; y: number }, drawRect: Rect): { x: number; y: number } {
  return { x: clamp(point.x, drawRect.x, drawRect.x + drawRect.width), y: clamp(point.y, drawRect.y, drawRect.y + drawRect.height) };
}

export function stageRectToPage(
  start: { x: number; y: number },
  end: { x: number; y: number },
  drawRect: Rect,
  frameMeta: FrameMetadata | null,
): PageRect | null {
  if (drawRect.width <= 0 || drawRect.height <= 0) return null;
  const deviceWidth = frameMeta?.deviceWidth || drawRect.width;
  const deviceHeight = frameMeta?.deviceHeight || drawRect.height;
  const scale = frameMeta?.pageScaleFactor || 1;
  const offsetTop = frameMeta?.offsetTop ?? 0;
  const scrollX = frameMeta?.scrollOffsetX ?? 0;
  const scrollY = frameMeta?.scrollOffsetY ?? 0;
  const toPage = (point: { x: number; y: number }) => {
    const local = clampToDraw(point, drawRect);
    const deviceX = ((local.x - drawRect.x) / drawRect.width) * deviceWidth;
    const deviceY = ((local.y - drawRect.y) / drawRect.height) * deviceHeight;
    return { x: deviceX / scale + scrollX, y: Math.max(0, deviceY - offsetTop) / scale + scrollY };
  };
  const a = toPage(start);
  const b = toPage(end);
  const x = Math.round(Math.min(a.x, b.x));
  const y = Math.round(Math.min(a.y, b.y));
  return { x, y, width: Math.round(Math.max(a.x, b.x)) - x, height: Math.round(Math.max(a.y, b.y)) - y };
}

export function viewportRequest(container: Size, dpr: number): StreamViewport | null {
  const w = Math.round(container.width);
  const h = Math.round(container.height);
  if (w <= 0 || h <= 0) return null;
  return { w, h, dpr: sanitizeDpr(dpr) };
}

export function requestedFrameSize(viewport: StreamViewport): Size {
  return backingSize({ width: viewport.w, height: viewport.h }, viewport.dpr);
}

export function sameViewport(a: StreamViewport | null, b: StreamViewport | null): boolean {
  if (!a || !b) return a === b;
  return a.w === b.w && a.h === b.h && a.dpr === b.dpr;
}

const FRAME_SIZE_TOLERANCE_PX = 1;

// Metadata without a device size cannot prove a mismatch, so it counts as matching.
export function frameMatchesViewport(meta: FrameMetadata | null | undefined, viewport: StreamViewport): boolean {
  if (!meta?.deviceWidth || !meta.deviceHeight) return true;
  return Math.abs(meta.deviceWidth - viewport.w) <= FRAME_SIZE_TOLERANCE_PX
    && Math.abs(meta.deviceHeight - viewport.h) <= FRAME_SIZE_TOLERANCE_PX;
}

export type ViewportAction = "connect" | "resize" | "none";

export function viewportAction(connected: boolean, lastSent: StreamViewport | null, next: StreamViewport | null): ViewportAction {
  if (!next) return "none";
  if (!connected) return "connect";
  return sameViewport(lastSent, next) ? "none" : "resize";
}

export function modifierMask(state: ModifierState): number {
  return (state.altKey ? MODIFIER.alt : 0)
    | (state.ctrlKey ? MODIFIER.ctrl : 0)
    | (state.metaKey ? MODIFIER.meta : 0)
    | (state.shiftKey ? MODIFIER.shift : 0);
}

function editingKeyName(event: KeyLike): string | null {
  if (event.code in VIRTUAL_KEY_CODES) return event.code;
  if (event.key in VIRTUAL_KEY_CODES) return event.key;
  return null;
}

function isEscape(event: KeyLike): boolean {
  return event.code === "Escape" || event.key === "Escape";
}

// Windows reports AltGr as Ctrl+Alt, so that pair still types a character.
function typesText(event: KeyLike): boolean {
  if ([...event.key].length !== 1) return false;
  if (event.metaKey) return false;
  return !event.ctrlKey || event.altKey;
}

export function keyInput(event: KeyLike): StreamInput | null {
  const name = editingKeyName(event);
  if (name) {
    const key = name === "NumpadEnter" ? "Enter" : name;
    const input: StreamInput = {
      kind: "key",
      key,
      code: event.code || key,
      windowsVirtualKeyCode: VIRTUAL_KEY_CODES[name],
      modifiers: modifierMask(event),
    };
    return key === "Enter" ? { ...input, text: "\r" } : input;
  }
  if (typesText(event)) return { kind: "text", text: event.key };
  return null;
}

export function decideKey(event: KeyLike, context: KeyContext): KeyDecision {
  if (isEscape(event) && event.shiftKey && context.interactive) return { action: "exit-interaction" };
  if (isEscape(event) && context.picking && context.origin === "surface") return { action: "cancel-pick" };
  if (context.origin === "editable" || !context.interactive || event.isComposing) return { action: "keep" };
  const input = keyInput(event);
  return input ? { action: "forward", input } : { action: "keep" };
}

export function clickInput(point: PagePoint, detail: number): StreamInput {
  return { kind: "click", x: point.x, y: point.y, button: "left", clickCount: Math.min(Math.max(1, detail), 3) };
}

export function wheelInput(
  point: PagePoint,
  delta: { deltaX: number; deltaY: number; deltaMode: number },
  pageHeight: number,
): StreamInput {
  const unit = delta.deltaMode === 1 ? WHEEL_LINE_PX : delta.deltaMode === 2 ? pageHeight : 1;
  return { kind: "wheel", x: point.x, y: point.y, deltaX: delta.deltaX * unit, deltaY: delta.deltaY * unit };
}

export function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}
