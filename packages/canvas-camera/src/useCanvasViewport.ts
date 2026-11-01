import { useCallback, useEffect, useRef, useState } from "react";
import { DEF_MIN_SCALE, DEF_MAX_SCALE, DEF_FIT_PADDING, FIT_MARGIN, FLY_DURATION_MS, GESTURE_SETTLE_MS, clamp, computeFitTransform, fitZoomFloor, flyTransform, type ViewportBox, type ViewportTransform } from "./viewportModel";
import { paintLayer } from "./paintLayer";
import { useCanvasWheel } from "./useCanvasWheel";
import { useCanvasTouch } from "./useCanvasTouch";
import { useCanvasMouse } from "./useCanvasMouse";


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
  touch?: boolean;
  constrainToFit?: boolean;
}

export interface FitOptions {
  /** A lower ceiling for this fit than the canvas's own maximum scale. */
  maxScale?: number;
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
  /** Fits `bounds`; an automatic fit passes `{ maxScale: AUTO_FIT_MAX_SCALE }` so it never zooms in. */
  fitToView: (bounds: ViewportBox, options?: FitOptions) => void;
  zoomBy: (delta: number) => void;
  isDragging: boolean;
  containerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Attach to the transformed world layer for immediate pinch/pan paint. */
  layerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Live camera, mutated every paint — read by the <canvas> blit loop. */
  transformRef: React.MutableRefObject<ViewportTransform>;
}

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
    touch = false,
    constrainToFit = true,
  } = opts;
  const onPaintRef = useRef(onPaint);
  onPaintRef.current = onPaint;

  const [transform, setTransformState] = useState<ViewportTransform>({ x: 0, y: 0, scale: 1 });

  const transformRef = useRef(transform);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const layerRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<number | null>(null);
  const trailRef = useRef<number | null>(null);
  const fitContentScaleRef = useRef<number | null>(null);

  const zoomFloor = useCallback(() => {
    const fit = fitContentScaleRef.current;
    if (!constrainToFit || fit == null || fit <= 0) return minScale;
    return fitZoomFloor(fit, minScale, FIT_MARGIN);
  }, [minScale, constrainToFit]);

  // must yield to direct manipulation instantly, never fight the pointer.
  const cancelAnim = useCallback(() => {
    if (animRef.current != null) {
      cancelAnimationFrame(animRef.current);
      animRef.current = null;
    }
  }, []);

  // frame writer — bypasses setTransform so it doesn't self-cancel the animation).
  const commit = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    paintLayer(layerRef.current, next);
    onPaintRef.current?.(next);
    if (trailRef.current != null) {
      clearTimeout(trailRef.current);
      trailRef.current = null;
    }
    setTransformState(next);
  }, []);

  const setTransform = useCallback((next: ViewportTransform) => {
    cancelAnim();
    commit(next);
  }, [cancelAnim, commit]);

  const schedule = useCallback((next: ViewportTransform) => {
    cancelAnim();
    transformRef.current = next;
    // Paint the camera to the layer EVERY event (cheap GPU transform — smooth pan/zoom).
    paintLayer(layerRef.current, next);
    onPaintRef.current?.(next);
    if (trailRef.current != null) clearTimeout(trailRef.current);
    trailRef.current = window.setTimeout(() => {
      trailRef.current = null;
      setTransformState(transformRef.current);
    }, GESTURE_SETTLE_MS);
  }, [cancelAnim]);

  const fitTransform = useCallback(
    (bounds: ViewportBox, cap = maxScale): ViewportTransform | null => {
      const t = computeFitTransform(bounds, width, height, { fitPadding, minScale, maxScale: Math.min(maxScale, cap) });
      if (t) fitContentScaleRef.current = t.scale;
      return t;
    },
    [width, height, fitPadding, minScale, maxScale],
  );

  const fitToView = useCallback(
    (bounds: ViewportBox, options?: FitOptions) => {
      const t = fitTransform(bounds, options?.maxScale);
      if (t) setTransform(t);
    },
    [fitTransform, setTransform],
  );

  const animateTo = useCallback(
    (next: ViewportTransform, durationMs = FLY_DURATION_MS) => {
      cancelAnim();
      if (trailRef.current != null) { clearTimeout(trailRef.current); trailRef.current = null; }
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
        const nextFrame = flyTransform(from, next, t);
        transformRef.current = nextFrame;
        paintLayer(layerRef.current, nextFrame);
        onPaintRef.current?.(nextFrame);
        if (t < 1) animRef.current = requestAnimationFrame(step);
        else { animRef.current = null; commit(next); }
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
      const floor = zoomFloor();
      const nextScale = clamp(t.scale * factor, floor, maxScale);
      if (nextScale === t.scale) return;
      const ratio = nextScale / t.scale;
      schedule({ scale: nextScale, x: px - (px - t.x) * ratio, y: py - (py - t.y) * ratio });
    },
    [maxScale, schedule, zoomFloor],
  );

  const zoomBy = useCallback(
    (delta: number) => zoomAround(1 + delta, width / 2, height / 2),
    [zoomAround, width, height],
  );

  useCanvasWheel(containerRef, transformRef, zoomAround, schedule, wheelBehavior);

  const { isDragging, onMouseDown } = useCanvasMouse(transformRef, cancelAnim, schedule, panButton, ignoreSelector);

  useCanvasTouch(containerRef, transformRef, touch, cancelAnim, schedule, zoomFloor, maxScale);

  useEffect(() => () => {
    if (trailRef.current != null) clearTimeout(trailRef.current);
  }, []);

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
