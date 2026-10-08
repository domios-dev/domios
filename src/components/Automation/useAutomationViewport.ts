import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { useCanvasViewport, viewportShowsBounds, type ViewportBox, type ViewportTransform } from "../Canvas/useCanvasViewport";
import { frameBlueprint, type BlueprintLayout, type CanvasSize } from "./blueprintLayout";

/** Camera and selection owned by the opener, so closing and reopening a Blueprint returns to the same view. */
export interface AutomationCamera {
  transform: ViewportTransform | null;
  userMoved: boolean;
  overview: boolean;
  focus: string | null;
  size: CanvasSize | null;
  selected: string | null;
}

export function createAutomationCamera(): AutomationCamera {
  return { transform: null, userMoved: false, overview: false, focus: null, size: null, selected: null };
}

const PAN_STEP = 64;
const KEY_PAN: Record<string, [number, number]> = { ArrowLeft: [PAN_STEP, 0], ArrowRight: [-PAN_STEP, 0], ArrowUp: [0, PAN_STEP], ArrowDown: [0, -PAN_STEP] };

function same(a: ViewportTransform, b: ViewportTransform): boolean {
  return a.x === b.x && a.y === b.y && a.scale === b.scale;
}

/** `focus` is the selected node a readable Fit frames; without it Fit frames the first stage. */
export function useAutomationViewport(layout: BlueprintLayout, camera: AutomationCamera, focus: string | null, panelOpen = false, onPaint?: (transform: ViewportTransform) => void) {
  const [size, setSize] = useState<CanvasSize | null>(null);
  const viewport = useCanvasViewport({ width: size?.width ?? 0, height: size?.height ?? 0, ignoreSelector: "button", wheelBehavior: "zoom", minScale: 0.2, onPaint });
  const { containerRef, setTransform, transformRef } = viewport;
  const fitted = useRef<ViewportTransform | null>(null);
  const moved = useRef(camera.userMoved);
  const overview = useRef(camera.overview);
  const framedFocus = useRef(camera.focus);
  const framedSize = useRef(camera.size);
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
      camera.overview = overview.current;
      camera.focus = framedFocus.current;
      camera.size = framedSize.current;
    };
  }, [camera, setTransform, transformRef, userMoved]);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const update = () => {
      const { clientWidth: width, clientHeight: height } = element;
      if (width && height) setSize((previous) => previous?.width === width && previous.height === height ? previous : { width, height });
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [containerRef]);

  const settle = useCallback((whole: boolean, target: string | null) => {
    if (!size) return;
    const transform = frameBlueprint(layout, size, whole, target);
    if (!transform) return;
    setTransform(transform);
    fitted.current = transform;
    moved.current = false;
    overview.current = whole;
    framedFocus.current = target;
  }, [layout, size, setTransform]);
  const fit = useCallback(() => settle(false, focus), [settle, focus]);
  const showOverview = useCallback(() => settle(true, null), [settle]);
  const panelWasOpen = useRef(false);
  useLayoutEffect(() => {
    if (panelOpen && !panelWasOpen.current && size && !userMoved()) settle(false, focus);
    panelWasOpen.current = panelOpen && size !== null;
  }, [panelOpen, size, settle, userMoved, focus]);

  // A moved camera keeps its scale and the world point at the viewport center when the canvas resizes.
  const follow = useRef(() => {});
  follow.current = () => {
    if (!size) return;
    const previous = framedSize.current;
    framedSize.current = size;
    if (!userMoved()) return settle(overview.current, framedFocus.current);
    const t = transformRef.current;
    const next = previous ? { ...t, x: t.x + (size.width - previous.width) / 2, y: t.y + (size.height - previous.height) / 2 } : t;
    if (!viewportShowsBounds(next, layout.bounds, size)) return settle(overview.current, framedFocus.current);
    if (!same(next, t)) setTransform(next);
    moved.current = true;
  };
  useLayoutEffect(() => follow.current(), [size?.width, size?.height, layout.bounds.width, layout.bounds.height]);

  const panBy = useCallback((dx: number, dy: number) => {
    const transform = transformRef.current;
    setTransform({ ...transform, x: transform.x + dx, y: transform.y + dy });
    moved.current = true;
  }, [setTransform, transformRef]);

  const reveal = useCallback((box: ViewportBox) => {
    if (!size) return;
    const t = transformRef.current;
    const left = box.x * t.scale + t.x;
    const top = box.y * t.scale + t.y;
    if (left >= 0 && top >= 0 && left + box.width * t.scale <= size.width && top + box.height * t.scale <= size.height) return;
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

  return { ...viewport, measured: size !== null, fit, showOverview, zoom, reveal, onKeyDown, touchHandlers };
}
