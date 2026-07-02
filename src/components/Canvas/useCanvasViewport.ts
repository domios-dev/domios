import { useCallback, useEffect, useRef, useState } from "react";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";

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

export type PanButton = "left" | "middle" | "any";

export interface UseCanvasViewportOptions {
  width: number;
  height: number;
  minScale?: number;
  maxScale?: number;
  fitPadding?: number;
  panButton?: PanButton;
  ignoreSelector?: string;
  /** Plain (no modifier) wheel: "pan" both axes (canvas) or "zoom" (mesh graph). */
  wheelBehavior?: "pan" | "zoom";
  /** Fired on every camera paint (gesture + fly) with the live transform — drives a
   *  <canvas> blit in lockstep with the layer paint, without a React commit. */
  onPaint?: (t: ViewportTransform) => void;
}

export interface CanvasViewportHandlers {
  onMouseDown: (e: React.MouseEvent) => void;
}

export interface UseCanvasViewportResult {
  transform: ViewportTransform;
  setTransform: (next: ViewportTransform) => void;
  /** Ease the camera to an exact transform (fly-to). Interrupted by any pan/zoom. */
  animateTo: (next: ViewportTransform, durationMs?: number) => void;
  /** Ease the camera so `bounds` fits the viewport (fly-to a node bbox). */
  animateToView: (bounds: ViewportBox, durationMs?: number) => void;
  handlers: CanvasViewportHandlers;
  fitToView: (bounds: ViewportBox) => void;
  zoomBy: (delta: number) => void;
  isDragging: boolean;
  containerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Attach to the transformed world layer for immediate pinch/pan paint. */
  layerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Live camera, mutated every paint — read by the <canvas> blit loop. */
  transformRef: React.MutableRefObject<ViewportTransform>;
}

const DEF_MIN_SCALE = DEFAULT_CANVAS_SETTINGS.zoomMinScale;
const DEF_MAX_SCALE = DEFAULT_CANVAS_SETTINGS.zoomMaxScale;
const DEF_FIT_PADDING = 28;
// Fit may zoom IN so a small board fills the viewport, but not past this so a
// single tiny plan doesn't blow up to fill the screen.
const FIT_MAX_ZOOM = DEFAULT_CANVAS_SETTINGS.zoomMaxScale;
// Fly-to camera ease: long enough to read as travel, short enough to feel instant.
const FLY_DURATION_MS = 460;
const LINE_PX = 16;
// Pinch / cmd-wheel zoom sensitivity. ~2.5x the old 0.0016 so a pinch feels
// immediate/Figma-like; the exponential keeps it smooth. Pan path is unchanged.
export const CANVAS_MIN_SCALE = DEF_MIN_SCALE;
export const CANVAS_MAX_SCALE = DEF_MAX_SCALE;
export const CANVAS_ZOOM_INTENSITY = 0.004;
const ZOOM_INTENSITY = CANVAS_ZOOM_INTENSITY;
// The layer transform paints every rAF (smooth 60fps camera); the React state
// commit that drives viewport-derived memos (culling/overlays/LOD) is throttled
// to ~15fps during a continuous gesture, with a trailing commit so culling
// settles exactly at gesture end. Decouples scene re-render from frame rate.
// Commit the React state (culling/overlays/LOD) only this long after the last
// pan/zoom event — i.e. once the gesture settles. During the gesture only the layer
// transform paints. Keeps WebKit/WKWebView from repainting the DOM/SVG mid-gesture.
const GESTURE_SETTLE_MS = 130;

export function formatLayerTransform(t: ViewportTransform): string {
  return `translate(${t.x}px, ${t.y}px) scale(${t.scale})`;
}

export function paintLayer(el: HTMLElement | null, t: ViewportTransform): void {
  if (!el) return;
  el.style.transform = formatLayerTransform(t);
  el.style.transformOrigin = "0 0";
}

// deltaMode: 0 pixel, 1 line, 2 page → normalize to pixels.
function pixelDelta(value: number, mode: number): number {
  if (mode === 1) return value * LINE_PX;
  if (mode === 2) return value * LINE_PX * 24;
  return value;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

// Figma-grade pan/zoom (§3): two-finger scroll pans, cmd/ctrl+scroll (and pinch,
// which the browser reports as ctrlKey) zooms around the pointer. Middle-drag and
// space+drag pan. Wheel is bound non-passive so preventDefault stops rubber-band.
export function useCanvasViewport(opts: UseCanvasViewportOptions): UseCanvasViewportResult {
  const {
    width,
    height,
    minScale = DEF_MIN_SCALE,
    maxScale = DEF_MAX_SCALE,
    fitPadding = DEF_FIT_PADDING,
    panButton = "any",
    ignoreSelector,
    wheelBehavior = "pan",
    onPaint,
  } = opts;
  const onPaintRef = useRef(onPaint);
  onPaintRef.current = onPaint;

  const [transform, setTransformState] = useState<ViewportTransform>({ x: 0, y: 0, scale: 1 });
  const [isDragging, setIsDragging] = useState(false);

  const transformRef = useRef(transform);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const layerRef = useRef<HTMLDivElement | null>(null);
  const spaceRef = useRef(false);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const frameRef = useRef<number | null>(null);
  const pendingRef = useRef<ViewportTransform | null>(null);
  const animRef = useRef<number | null>(null);
  // Throttle bookkeeping for the React state commit (see STATE_COMMIT_THROTTLE_MS).
  const lastStateTsRef = useRef(0);
  const trailRef = useRef<number | null>(null);
  // §35 scroll-trap escape state: the scroll-body currently caught at its boundary
  // and whether it has already broken free to canvas pan, plus the timestamp of the
  // last wheel tick (to tell a fresh deliberate gesture from a momentum-fling tail).
  const boundaryArmRef = useRef<{ el: HTMLElement; released: boolean } | null>(null);
  const lastWheelTsRef = useRef(0);

  // Stop any in-flight fly-to. Called whenever the user takes the wheel — a fly
  // must yield to direct manipulation instantly, never fight the pointer.
  const cancelAnim = useCallback(() => {
    if (animRef.current != null) {
      cancelAnimationFrame(animRef.current);
      animRef.current = null;
    }
  }, []);

  // Commit a transform to ref + layer paint + React state in one shot (the fly-to
  // frame writer — bypasses setTransform so it doesn't self-cancel the animation).
  const commit = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    paintLayer(layerRef.current, next);
    onPaintRef.current?.(next);
    if (trailRef.current != null) {
      clearTimeout(trailRef.current);
      trailRef.current = null;
    }
    lastStateTsRef.current = performance.now();
    setTransformState(next);
  }, []);

  const setTransform = useCallback((next: ViewportTransform) => {
    cancelAnim();
    commit(next);
  }, [cancelAnim, commit]);

  const schedule = useCallback((next: ViewportTransform) => {
    cancelAnim();
    transformRef.current = next;
    pendingRef.current = next;
    // Paint the camera to the layer EVERY event (cheap GPU transform — smooth pan/zoom).
    paintLayer(layerRef.current, next);
    onPaintRef.current?.(next);
    // Defer the React commit (culling/overlays/LOD) to gesture-END only: re-arm a
    // debounce on each event so we commit ONCE when the gesture settles. WebKit
    // repaints the visible DOM/SVG ~hundreds of ms per commit; committing mid-gesture
    // is the pan/zoom jank. During the gesture the layer transform alone moves the scene.
    if (trailRef.current != null) clearTimeout(trailRef.current);
    trailRef.current = window.setTimeout(() => {
      trailRef.current = null;
      setTransformState(transformRef.current);
    }, GESTURE_SETTLE_MS);
  }, [cancelAnim]);

  const fitTransform = useCallback(
    (bounds: ViewportBox): ViewportTransform | null => {
      if (width <= 0 || height <= 0 || bounds.width <= 0 || bounds.height <= 0) return null;
      const scaleX = (width - fitPadding * 2) / bounds.width;
      const scaleY = (height - fitPadding * 2) / bounds.height;
      const scale = clamp(Math.min(scaleX, scaleY, FIT_MAX_ZOOM), minScale, maxScale);
      const cx = bounds.x + bounds.width / 2;
      const cy = bounds.y + bounds.height / 2;
      return { x: width / 2 - cx * scale, y: height / 2 - cy * scale, scale };
    },
    [width, height, fitPadding, minScale, maxScale],
  );

  const fitToView = useCallback(
    (bounds: ViewportBox) => {
      const t = fitTransform(bounds);
      if (t) setTransform(t);
    },
    [fitTransform, setTransform],
  );

  // Ease the camera to an exact transform. Reduced-motion (or a zero delta) lands
  // instantly; otherwise an ease-out-cubic rAF loop drives ref+paint+state together
  // so every layer (canvas, overlays, world DOM) flies in lockstep.
  const animateTo = useCallback(
    (next: ViewportTransform, durationMs = FLY_DURATION_MS) => {
      cancelAnim();
      const from = transformRef.current;
      const dx = next.x - from.x;
      const dy = next.y - from.y;
      const ds = next.scale - from.scale;
      const reduce =
        typeof window !== "undefined" &&
        typeof window.matchMedia === "function" &&
        window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (durationMs <= 0 || reduce || (dx === 0 && dy === 0 && ds === 0)) {
        commit(next);
        return;
      }
      let start: number | null = null;
      const step = (ts: number) => {
        if (start == null) start = ts;
        const t = Math.min(1, (ts - start) / durationMs);
        const e = 1 - Math.pow(1 - t, 3);
        commit({ x: from.x + dx * e, y: from.y + dy * e, scale: from.scale + ds * e });
        if (t < 1) animRef.current = requestAnimationFrame(step);
        else animRef.current = null;
      };
      animRef.current = requestAnimationFrame(step);
    },
    [cancelAnim, commit],
  );

  const animateToView = useCallback(
    (bounds: ViewportBox, durationMs = FLY_DURATION_MS) => {
      const t = fitTransform(bounds);
      if (t) animateTo(t, durationMs);
    },
    [fitTransform, animateTo],
  );

  useEffect(() => cancelAnim, [cancelAnim]);

  const zoomAround = useCallback(
    (factor: number, px: number, py: number) => {
      const t = transformRef.current;
      const nextScale = clamp(t.scale * factor, minScale, maxScale);
      if (nextScale === t.scale) return;
      const ratio = nextScale / t.scale;
      schedule({ scale: nextScale, x: px - (px - t.x) * ratio, y: py - (py - t.y) * ratio });
    },
    [minScale, maxScale, schedule],
  );

  const zoomBy = useCallback(
    (delta: number) => zoomAround(1 + delta, width / 2, height / 2),
    [zoomAround, width, height],
  );

  // Native non-passive wheel: pan on plain scroll, zoom-around-pointer on cmd/ctrl/pinch.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const rect = el.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey || wheelBehavior === "zoom") {
        e.preventDefault();
        const dy = pixelDelta(e.deltaY, e.deltaMode);
        zoomAround(Math.exp(-dy * ZOOM_INTENSITY), e.clientX - rect.left, e.clientY - rect.top);
        return;
      }
      const dx = pixelDelta(e.deltaX, e.deltaMode);
      const dy = pixelDelta(e.deltaY, e.deltaMode);
      const gap = e.timeStamp - lastWheelTsRef.current;
      lastWheelTsRef.current = e.timeStamp;
      const newGesture = gap > 220; // a deliberate re-scroll, not a momentum tail
      // §35 scroll-trap escape: a node body owns the VERTICAL wheel while it can still
      // scroll that way. When it hits the top/bottom boundary the first ticks are
      // ABSORBED (a rubber-band beat — content "catches" at the edge); only the NEXT
      // deliberate scroll in that direction breaks free to canvas pan. A sideways
      // (horizontal-dominant) wheel always pans — a second exit hatch. Pinch/⌘-zoom
      // (handled above) is never trapped.
      const body = (e.target as HTMLElement | null)?.closest?.("[data-canvas-scroll-body]") as HTMLElement | null;
      const sideways = Math.abs(dx) > Math.abs(dy);
      if (body && !sideways) {
        const canScroll = body.scrollHeight - body.clientHeight > 1;
        const dir = Math.sign(dy);
        const atBoundary =
          (dir < 0 && body.scrollTop <= 0) ||
          (dir > 0 && body.scrollTop + body.clientHeight >= body.scrollHeight - 1);
        if (canScroll && !atBoundary) {
          boundaryArmRef.current = null; // scrolling within content → disarm
          return; // node native-scrolls; no pan
        }
        if (canScroll && atBoundary) {
          const arm = boundaryArmRef.current;
          if (arm?.el === body && arm.released) {
            // already broke free → keep panning the canvas (no stutter / re-arm)
          } else if (arm?.el === body && newGesture) {
            boundaryArmRef.current = { el: body, released: true }; // fresh scroll → release
          } else {
            boundaryArmRef.current = { el: body, released: false }; // rubber-band beat
            e.preventDefault();
            return;
          }
        }
        // a non-scrollable body falls through to canvas pan immediately
      } else {
        boundaryArmRef.current = null;
      }
      e.preventDefault();
      const t = transformRef.current;
      schedule({ ...t, x: t.x - dx, y: t.y - dy });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomAround, schedule, wheelBehavior]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space") spaceRef.current = true;
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") spaceRef.current = false;
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, []);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      const middle = e.button === 1;
      const spaceLeft = e.button === 0 && spaceRef.current;
      const allowed = middle || spaceLeft || panButton === "any";
      if (!allowed) return;
      if (ignoreSelector && (e.target as HTMLElement).closest(ignoreSelector)) return;
      cancelAnim();
      dragRef.current = {
        startX: e.clientX,
        startY: e.clientY,
        baseX: transformRef.current.x,
        baseY: transformRef.current.y,
      };
      setIsDragging(true);
    },
    [panButton, ignoreSelector, cancelAnim],
  );

  useEffect(() => {
    if (!isDragging) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      schedule({ ...transformRef.current, x: d.baseX + (e.clientX - d.startX), y: d.baseY + (e.clientY - d.startY) });
    };
    const onUp = () => {
      dragRef.current = null;
      setIsDragging(false);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (frameRef.current != null) {
        cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
      if (trailRef.current != null) {
        clearTimeout(trailRef.current);
        trailRef.current = null;
      }
    };
  }, [isDragging, schedule]);

  return {
    transform,
    setTransform,
    animateTo,
    animateToView,
    handlers: { onMouseDown },
    fitToView,
    zoomBy,
    isDragging,
    containerRef,
    layerRef,
    transformRef,
  };
}
