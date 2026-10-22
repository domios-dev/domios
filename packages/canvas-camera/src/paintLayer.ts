import { formatLayerTransform, type ViewportTransform } from "./viewportModel";
const RASTER_SETTLE_MS = 180;
const rasterSettleTimers = new WeakMap<HTMLElement, number>();

export function paintLayer(el: HTMLElement | null, t: ViewportTransform): void {
  if (!el) return;
  el.style.transform = formatLayerTransform(t);
  el.style.transformOrigin = "0 0";
  // re-rasterize text at the final scale instead of stretching a 1x bitmap.
  el.style.willChange = "transform";
  const prev = rasterSettleTimers.get(el);
  if (prev !== undefined) window.clearTimeout(prev);
  rasterSettleTimers.set(
    el,
    window.setTimeout(() => {
      el.style.willChange = "auto";
      rasterSettleTimers.delete(el);
    }, RASTER_SETTLE_MS),
  );
}
