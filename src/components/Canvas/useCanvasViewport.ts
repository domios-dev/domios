import { useCallback, useEffect, useRef, useState } from "react";

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
}

export interface CanvasViewportHandlers {
  onMouseDown: (e: React.MouseEvent) => void;
}

export interface UseCanvasViewportResult {
  transform: ViewportTransform;
  setTransform: (next: ViewportTransform) => void;
  handlers: CanvasViewportHandlers;
  fitToView: (bounds: ViewportBox) => void;
  zoomBy: (delta: number) => void;
  isDragging: boolean;
  containerRef: React.MutableRefObject<HTMLDivElement | null>;
}

const DEF_MIN_SCALE = 0.08;
const DEF_MAX_SCALE = 2.5;
const DEF_FIT_PADDING = 28;
// Fit may zoom IN so a small board fills the viewport, but not past this so a
// single tiny plan doesn't blow up to fill the screen.
const FIT_MAX_ZOOM = 2.4;
const LINE_PX = 16;
const ZOOM_INTENSITY = 0.0016;

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
  } = opts;

  const [transform, setTransformState] = useState<ViewportTransform>({ x: 0, y: 0, scale: 1 });
  const [isDragging, setIsDragging] = useState(false);

  const transformRef = useRef(transform);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const spaceRef = useRef(false);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const frameRef = useRef<number | null>(null);
  const pendingRef = useRef<ViewportTransform | null>(null);

  const setTransform = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    setTransformState(next);
  }, []);

  const schedule = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    pendingRef.current = next;
    if (frameRef.current != null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const pending = pendingRef.current;
      pendingRef.current = null;
      if (pending) setTransformState(pending);
    });
  }, []);

  const fitToView = useCallback(
    (bounds: ViewportBox) => {
      if (width <= 0 || height <= 0 || bounds.width <= 0 || bounds.height <= 0) return;
      const scaleX = (width - fitPadding * 2) / bounds.width;
      const scaleY = (height - fitPadding * 2) / bounds.height;
      const scale = clamp(Math.min(scaleX, scaleY, FIT_MAX_ZOOM), minScale, maxScale);
      const cx = bounds.x + bounds.width / 2;
      const cy = bounds.y + bounds.height / 2;
      setTransform({ x: width / 2 - cx * scale, y: height / 2 - cy * scale, scale });
    },
    [width, height, fitPadding, minScale, maxScale, setTransform],
  );

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
      e.preventDefault();
      const rect = el.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey || wheelBehavior === "zoom") {
        const dy = pixelDelta(e.deltaY, e.deltaMode);
        zoomAround(Math.exp(-dy * ZOOM_INTENSITY), e.clientX - rect.left, e.clientY - rect.top);
        return;
      }
      const dx = pixelDelta(e.deltaX, e.deltaMode);
      const dy = pixelDelta(e.deltaY, e.deltaMode);
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
      dragRef.current = {
        startX: e.clientX,
        startY: e.clientY,
        baseX: transformRef.current.x,
        baseY: transformRef.current.y,
      };
      setIsDragging(true);
    },
    [panButton, ignoreSelector],
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
    };
  }, [isDragging, schedule]);

  return {
    transform,
    setTransform,
    handlers: { onMouseDown },
    fitToView,
    zoomBy,
    isDragging,
    containerRef,
  };
}
