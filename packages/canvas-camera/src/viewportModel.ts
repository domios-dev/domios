export interface ViewportTransform {
  x: number;
  y: number;
  scale: number;
}

export interface ViewportBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const DEF_MIN_SCALE = 0.1;
export const DEF_MAX_SCALE = 4;
/** An automatic fit only zooms out: a small layout (a card still loading, a sparse graph) shows at its natural size
 *  rather than blown up. A fit the viewer asks for may zoom in up to the canvas's own maximum. */
export const AUTO_FIT_MAX_SCALE = 1;
export const DEF_FIT_PADDING = 28;
export const FIT_MAX_ZOOM = 4;
export const FIT_MARGIN = 0.1;
// Fly-to camera ease: long enough to read as travel, short enough to feel instant.
export const FLY_DURATION_MS = 460;
// immediate/Figma-like; the exponential keeps it smooth. Pan path is unchanged.
export const CANVAS_MIN_SCALE = DEF_MIN_SCALE;
export const CANVAS_MAX_SCALE = DEF_MAX_SCALE;
export const CANVAS_FIT_MARGIN = FIT_MARGIN;
export const CANVAS_ZOOM_INTENSITY = 0.004;
export const GESTURE_SETTLE_MS = 130;

export function formatLayerTransform(t: ViewportTransform): string {
  return `translate(${t.x}px, ${t.y}px) scale(${t.scale})`;
}


export function clamp(v: number, lo: number, hi: number): number { return Math.max(lo, Math.min(hi, v)); }

export function computeFitTransform(
  bounds: ViewportBox,
  width: number,
  height: number,
  opts: { fitPadding?: number; minScale?: number; maxScale?: number } = {},
): ViewportTransform | null {
  const fitPadding = opts.fitPadding ?? DEF_FIT_PADDING;
  const minScale = opts.minScale ?? DEF_MIN_SCALE;
  const maxScale = opts.maxScale ?? DEF_MAX_SCALE;
  if (width <= 0 || height <= 0 || bounds.width <= 0 || bounds.height <= 0) return null;
  const scaleX = (width - fitPadding * 2) / bounds.width;
  const scaleY = (height - fitPadding * 2) / bounds.height;
  const raw = Math.min(scaleX, scaleY, FIT_MAX_ZOOM);
  const scale = clamp(raw, minScale, maxScale);
  const cx = bounds.x + bounds.width / 2;
  const cy = bounds.y + bounds.height / 2;
  return { x: width / 2 - cx * scale, y: height / 2 - cy * scale, scale };
}

export function fitZoomFloor(fitScale: number, minScale = DEF_MIN_SCALE, margin = FIT_MARGIN): number {
  return Math.max(minScale, fitScale * (1 - margin));
}

export function viewportShowsBounds(t: ViewportTransform, bounds: ViewportBox, viewport: { width: number; height: number }): boolean {
  const vx = -t.x / t.scale;
  const vy = -t.y / t.scale;
  const vw = viewport.width / t.scale;
  const vh = viewport.height / t.scale;
  const ix = Math.max(0, Math.min(bounds.x + bounds.width, vx + vw) - Math.max(bounds.x, vx));
  const iy = Math.max(0, Math.min(bounds.y + bounds.height, vy + vh) - Math.max(bounds.y, vy));
  return ix > 0 && iy > 0;
}


export function flyTransform(from: ViewportTransform, to: ViewportTransform, progress: number): ViewportTransform {
  const ease = 1 - Math.pow(1 - clamp(progress, 0, 1), 3);
  return { x: from.x + (to.x - from.x) * ease, y: from.y + (to.y - from.y) * ease, scale: from.scale + (to.scale - from.scale) * ease };
}
