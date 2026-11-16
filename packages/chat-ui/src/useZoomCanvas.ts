import { useLayoutEffect, useRef, useState, type HTMLAttributes } from "react";
import { type ViewportBox } from "@codeterm/canvas-camera";
import { swipeDirection } from "./galleryModel";
import { type InitialCanvasFit } from "./imageCanvasCamera";
import { useCanvasStage } from "./useCanvasStage";
import { canvasGrid } from "./canvasGrid";
import { nextCameraRest, type CameraRest, type CameraRestEvent } from "./canvasRest";

export interface CanvasFocus { request: number; bounds: ViewportBox }
type ViewMode = "fit" | "focus" | "custom" | "natural";

export function useZoomCanvas({ bounds, focus, cameraKey, initialFit, wheelBehavior, onNavigate, onSelect }: {
  bounds: ViewportBox;
  focus?: CanvasFocus | null;
  cameraKey?: string;
  initialFit: InitialCanvasFit;
  wheelBehavior: "pan" | "zoom";
  onNavigate?: (direction: number) => void;
  onSelect?: (index: number) => void;
}) {
  const [mode, setMode] = useState<ViewMode>("fit");
  const focusRequest = useRef<number | null>(null);
  const scaleOutput = useRef<HTMLOutputElement>(null);
  const imageCamera = cameraKey !== undefined;
  const [rest, setRest] = useState<CameraRest>(null);
  const restRef = useRef<CameraRest>(null);
  const moveRest = (event: CameraRestEvent) => {
    const next = nextCameraRest(restRef.current, event);
    if (next === restRef.current) return;
    restRef.current = next;
    setRest(next);
  };
  const stage = useCanvasStage({ minScale: 0.0001, maxScale: 8, wheelBehavior, constrainToFit: false,
    memoryKey: cameraKey === undefined ? undefined : `image:${cameraKey}`,
    ready: initialFit === "ready", cancelled: initialFit === "cancelled",
    followFit: !imageCamera,
    fitRevision: `${bounds.x}:${bounds.y}:${bounds.width}:${bounds.height}`,
    fit: (camera, _size, initial) => { camera.fitToView(initial ? focus?.bounds ?? bounds : bounds); if (!initial) setMode("fit"); else if (focus) setMode("custom"); },
    onUserControl: () => setMode("custom"), onReset: () => natural(), onNavigate,
    onPaint: transform => {
      moveRest({ kind: "paint", scale: transform.scale });
      if (scaleOutput.current) scaleOutput.current.textContent = `${Math.round(transform.scale * 100)}%`;
      const style = camera.containerRef.current?.style;
      const grid = canvasGrid(transform);
      style?.setProperty("--canvas-step", `${grid.spacing}px`);
      style?.setProperty("--canvas-dot", `${grid.radius}px`);
      style?.setProperty("--canvas-x", `${grid.x}px`);
      style?.setProperty("--canvas-y", `${grid.y}px`);
    },
  });
  const { camera, size } = stage;
  const { animateToView } = camera;
  const previousSize = useRef(size);
  useLayoutEffect(() => moveRest({ kind: "settle", scale: camera.transform.scale }), [camera.transform]);
  useLayoutEffect(() => { if (imageCamera && stage.restored) setMode("custom"); }, [imageCamera, stage.restored]);
  useLayoutEffect(() => {
    const previous = previousSize.current;
    if (imageCamera && mode === "fit" && previous.width > 0 && (previous.width !== size.width || previous.height !== size.height)) setMode("custom");
    previousSize.current = size;
  }, [imageCamera, mode, size]);
  const touch = useRef<{ x: number; y: number; time: number; pinched: boolean } | null>(null);
  const lastTap = useRef<{ x: number; y: number; time: number } | null>(null);
  const mouseStart = useRef<{ x: number; y: number } | null>(null);
  const handledTap = useRef<{ x: number; y: number; time: number } | null>(null);
  const changeMode = (next: ViewMode) => { stage.takeControl(); setMode(next); };
  const fit = () => {
    changeMode("fit");
    stage.fit();
  };
  const zoom = (delta: number) => { changeMode("custom"); camera.zoomBy(delta); };
  const natural = () => {
    changeMode("natural");
    camera.setTransform({ x: size.width / 2 - bounds.x - bounds.width / 2, y: size.height / 2 - bounds.y - bounds.height / 2, scale: 1 });
  };
  useLayoutEffect(() => {
    if (imageCamera || size.width <= 0 || size.height <= 0 || !focus || focus.request === focusRequest.current) return;
    focusRequest.current = focus.request;
    changeMode("focus");
    animateToView(focus.bounds);
  });
  const interactions: HTMLAttributes<HTMLDivElement> = {
    onWheelCapture: () => changeMode("custom"),
    onMouseDownCapture: event => { changeMode("custom"); mouseStart.current = { x: event.clientX, y: event.clientY }; },
    onClickCapture: event => {
      const start = mouseStart.current;
      const tap = handledTap.current;
      const afterTap = tap && event.detail > 0 && event.timeStamp - tap.time < 800 && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) < 30;
      if (afterTap || start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) {
        event.preventDefault(); event.stopPropagation();
      }
      handledTap.current = null;
      mouseStart.current = null;
    },
    onDoubleClick: imageCamera ? undefined : fit,
    onTouchStartCapture: () => changeMode("custom"),
    onTouchStart: event => {
      if (event.touches.length > 1) { if (touch.current) touch.current.pinched = true; return; }
      const point = event.touches[0];
      touch.current = { x: point.clientX, y: point.clientY, time: event.timeStamp, pinched: false };
    },
    onTouchCancel: () => { touch.current = null; lastTap.current = null; },
    onTouchEnd: event => {
      const start = touch.current;
      if (!start || event.touches.length) return;
      touch.current = null;
      const point = event.changedTouches[0];
      if (!point) return;
      const dx = point.clientX - start.x, dy = point.clientY - start.y;
      const direction = swipeDirection(dx, dy, event.timeStamp - start.time, start.pinched);
      if (direction) onNavigate?.(direction);
      if (!start.pinched && Math.hypot(dx, dy) < 12) {
        handledTap.current = { x: point.clientX, y: point.clientY, time: event.timeStamp };
        const last = lastTap.current;
        if (!imageCamera && last && event.timeStamp - last.time < 320 && Math.hypot(point.clientX - last.x, point.clientY - last.y) < 30) {
          fit(); lastTap.current = null;
        } else {
          lastTap.current = { x: point.clientX, y: point.clientY, time: event.timeStamp };
          const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-zoom-target]") : null;
          if (target && target.dataset.zoomTarget !== undefined) onSelect?.(Number(target.dataset.zoomTarget));
        }
      }
    },
    onKeyDown: stage.interactions.onKeyDown,
  };
  return { camera, mode, rest, scaleOutput, interactions, fit, zoom, natural };
}
