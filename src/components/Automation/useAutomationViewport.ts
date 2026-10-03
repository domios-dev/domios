import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { computeFitTransform, useCanvasViewport, type ViewportBox, type ViewportTransform } from "../Canvas/useCanvasViewport";

/** Camera owned by the scenario surface so a list toggle or data refresh never resets the user's view. */
export interface AutomationCamera {
  transform: ViewportTransform | null;
  userMoved: boolean;
}

export function createAutomationCamera(): AutomationCamera {
  return { transform: null, userMoved: false };
}

const PAN_STEP = 64;
const KEY_PAN: Record<string, [number, number]> = { ArrowLeft: [PAN_STEP, 0], ArrowRight: [-PAN_STEP, 0], ArrowUp: [0, PAN_STEP], ArrowDown: [0, -PAN_STEP] };

function same(a: ViewportTransform, b: ViewportTransform): boolean {
  return a.x === b.x && a.y === b.y && a.scale === b.scale;
}

export function useAutomationViewport(bounds: ViewportBox, camera: AutomationCamera) {
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const viewport = useCanvasViewport({ width: size?.width ?? 0, height: size?.height ?? 0, ignoreSelector: "button", wheelBehavior: "zoom" });
  const { containerRef, setTransform, transformRef } = viewport;
  const fitted = useRef<ViewportTransform | null>(null);
  const moved = useRef(camera.userMoved);
  const touch = useRef<{ id: number; x: number; y: number; base: ViewportTransform } | null>(null);
  const userMoved = useCallback(() => moved.current || (fitted.current !== null && !same(transformRef.current, fitted.current)), [transformRef]);

  useLayoutEffect(() => {
    if (camera.transform) {
      setTransform(camera.transform);
      if (!camera.userMoved) fitted.current = camera.transform;
    }
    return () => {
      camera.transform = transformRef.current;
      camera.userMoved = userMoved();
    };
  }, [camera, setTransform, transformRef, userMoved]);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const update = () => {
      const { clientWidth: width, clientHeight: height } = element;
      if (width && height) setSize((current) => current?.width === width && current.height === height ? current : { width, height });
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [containerRef]);

  const fit = useCallback(() => {
    if (!size) return;
    const next = computeFitTransform(bounds, size.width, size.height, { maxScale: 1 });
    if (!next) return;
    setTransform(next);
    fitted.current = next;
    moved.current = false;
  }, [bounds, size, setTransform]);

  // Auto-fit only while the camera is untouched: first measurement, resize, or a changed graph extent.
  const autoFit = useRef(fit);
  autoFit.current = () => { if (!userMoved()) fit(); };
  useLayoutEffect(() => autoFit.current(), [size?.width, size?.height, bounds.width, bounds.height]);

  const panBy = useCallback((dx: number, dy: number) => {
    const current = transformRef.current;
    setTransform({ ...current, x: current.x + dx, y: current.y + dy });
    moved.current = true;
  }, [setTransform, transformRef]);

  const reveal = useCallback((box: ViewportBox) => {
    if (!size) return;
    const t = transformRef.current;
    const left = box.x * t.scale + t.x;
    const top = box.y * t.scale + t.y;
    const inside = left >= 0 && top >= 0 && left + box.width * t.scale <= size.width && top + box.height * t.scale <= size.height;
    if (inside) return;
    setTransform({ ...t, x: size.width / 2 - (box.x + box.width / 2) * t.scale, y: size.height / 2 - (box.y + box.height / 2) * t.scale });
    moved.current = true;
  }, [setTransform, size, transformRef]);

  const zoom = useCallback((delta: number) => {
    viewport.zoomBy(delta);
    moved.current = true;
  }, [viewport]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    const pan = KEY_PAN[event.code];
    if (pan) { event.preventDefault(); panBy(pan[0], pan[1]); return; }
    if (event.code === "Equal" || event.code === "NumpadAdd") { event.preventDefault(); zoom(0.2); }
    else if (event.code === "Minus" || event.code === "NumpadSubtract") { event.preventDefault(); zoom(-0.2); }
    else if (event.code === "Digit0" || event.code === "Numpad0") { event.preventDefault(); fit(); }
  };

  // Mouse panning is owned by useCanvasViewport; touch drags pan here so phones are not stuck at the fitted view.
  const touchHandlers = {
    onPointerDown: (event: React.PointerEvent<HTMLElement>) => {
      if (event.pointerType === "mouse" || (event.target as HTMLElement).closest("button")) return;
      touch.current = { id: event.pointerId, x: event.clientX, y: event.clientY, base: transformRef.current };
    },
    onPointerMove: (event: React.PointerEvent<HTMLElement>) => {
      const start = touch.current;
      if (!start || start.id !== event.pointerId) return;
      setTransform({ ...start.base, x: start.base.x + event.clientX - start.x, y: start.base.y + event.clientY - start.y });
      moved.current = true;
    },
    onPointerUp: () => { touch.current = null; },
    onPointerCancel: () => { touch.current = null; },
  };

  return { ...viewport, measured: size !== null, fit, zoom, reveal, onKeyDown, touchHandlers };
}
