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
  zoomStep?: number;
  fitPadding?: number;
  panButton?: PanButton;
  ignoreSelector?: string;
}

export interface CanvasViewportHandlers {
  onMouseDown: (e: React.MouseEvent) => void;
  onWheel: (e: React.WheelEvent) => void;
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

const DEF_MIN_SCALE = 0.1;
const DEF_MAX_SCALE = 2.5;
const DEF_ZOOM_STEP = 0.06;
const DEF_FIT_PADDING = 80;

function buttonAllowed(button: number, mode: PanButton): boolean {
  if (mode === "any") return true;
  if (mode === "middle") return button === 1;
  return button === 0;
}

// Shared pan/zoom for absolutely-positioned canvases (§6 decision 2). Drag to pan,
// wheel to zoom around the cursor; fitToView frames a content bbox.
export function useCanvasViewport(opts: UseCanvasViewportOptions): UseCanvasViewportResult {
  const {
    width,
    height,
    minScale = DEF_MIN_SCALE,
    maxScale = DEF_MAX_SCALE,
    zoomStep = DEF_ZOOM_STEP,
    fitPadding = DEF_FIT_PADDING,
    panButton = "any",
    ignoreSelector,
  } = opts;

  const [transform, setTransformState] = useState<ViewportTransform>({ x: 0, y: 0, scale: 1 });
  const [isDragging, setIsDragging] = useState(false);

  const transformRef = useRef(transform);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const dragFrameRef = useRef<number | null>(null);
  const pendingRef = useRef<ViewportTransform | null>(null);

  const setTransform = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    setTransformState(next);
  }, []);

  const scheduleTransform = useCallback((next: ViewportTransform) => {
    transformRef.current = next;
    pendingRef.current = next;
    if (dragFrameRef.current != null) return;
    dragFrameRef.current = requestAnimationFrame(() => {
      dragFrameRef.current = null;
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
      const scale = Math.max(minScale, Math.min(maxScale, Math.min(scaleX, scaleY, 1)));
      const cxModel = bounds.x + bounds.width / 2;
      const cyModel = bounds.y + bounds.height / 2;
      setTransform({
        x: width / 2 - cxModel * scale,
        y: height / 2 - cyModel * scale,
        scale,
      });
    },
    [width, height, fitPadding, minScale, maxScale, setTransform],
  );

  const zoomBy = useCallback(
    (delta: number) => {
      const t = transformRef.current;
      const nextScale = Math.max(minScale, Math.min(maxScale, t.scale + delta));
      if (nextScale === t.scale) return;
      setTransform({ ...t, scale: nextScale });
    },
    [minScale, maxScale, setTransform],
  );

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (!buttonAllowed(e.button, panButton)) return;
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
      scheduleTransform({
        ...transformRef.current,
        x: d.baseX + (e.clientX - d.startX),
        y: d.baseY + (e.clientY - d.startY),
      });
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
      if (dragFrameRef.current != null) {
        cancelAnimationFrame(dragFrameRef.current);
        dragFrameRef.current = null;
      }
    };
  }, [isDragging, scheduleTransform]);

  const onWheel = useCallback(
    (e: React.WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY < 0 ? zoomStep : -zoomStep;
      const t = transformRef.current;
      const nextScale = Math.max(minScale, Math.min(maxScale, t.scale + delta));
      if (nextScale === t.scale) return;
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) {
        setTransform({ ...t, scale: nextScale });
        return;
      }
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      const ratio = nextScale / t.scale;
      setTransform({
        scale: nextScale,
        x: cx - (cx - t.x) * ratio,
        y: cy - (cy - t.y) * ratio,
      });
    },
    [zoomStep, minScale, maxScale, setTransform],
  );

  return {
    transform,
    setTransform,
    handlers: { onMouseDown, onWheel },
    fitToView,
    zoomBy,
    isDragging,
    containerRef,
  };
}
