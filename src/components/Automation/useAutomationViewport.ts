import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useCanvasViewport, viewportShowsBounds, type ViewportBox, type ViewportTransform } from "../Canvas/useCanvasViewport";
import { layoutAutomationOrientations, planAutomationView, type AutomationOrientation, type CanvasSize } from "./layout";
import type { AutomationGraph } from "./types";

/** Camera owned by the scenario surface so a list toggle or data refresh never resets the user's view. */
export interface AutomationCamera {
  transform: ViewportTransform | null;
  userMoved: boolean;
  orientation: AutomationOrientation | null;
  wrap: number | null;
  overview: boolean;
  focus: string | null;
  size: CanvasSize | null;
}

export function createAutomationCamera(): AutomationCamera {
  return { transform: null, userMoved: false, orientation: null, wrap: null, overview: false, focus: null, size: null };
}

const PAN_STEP = 64;
const KEY_PAN: Record<string, [number, number]> = { ArrowLeft: [PAN_STEP, 0], ArrowRight: [-PAN_STEP, 0], ArrowUp: [0, PAN_STEP], ArrowDown: [0, -PAN_STEP] };

function same(a: ViewportTransform, b: ViewportTransform): boolean {
  return a.x === b.x && a.y === b.y && a.scale === b.scale;
}

interface Arrangement { orientation: AutomationOrientation; wrap: number }

/** `focus` is the selected participant that a readable Fit frames; without it Fit frames the root. */
export function useAutomationViewport(graph: AutomationGraph, camera: AutomationCamera, focus: string | null, panelOpen = false) {
  const [size, setSize] = useState<CanvasSize | null>(null);
  const [arrangement, setArrangement] = useState<Arrangement>({ orientation: camera.orientation ?? "horizontal", wrap: camera.wrap ?? Number.POSITIVE_INFINITY });
  const layouts = useMemo(() => layoutAutomationOrientations(graph, arrangement.wrap), [graph, arrangement.wrap]);
  const { orientation } = arrangement;
  const viewport = useCanvasViewport({ width: size?.width ?? 0, height: size?.height ?? 0, ignoreSelector: "button", wheelBehavior: "zoom" });
  const { containerRef, setTransform, transformRef } = viewport;
  const fitted = useRef<ViewportTransform | null>(null);
  const moved = useRef(camera.userMoved);
  const overview = useRef(camera.overview);
  const framedFocus = useRef(camera.focus);
  const current = useRef(arrangement);
  current.current = arrangement;
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
      camera.orientation = current.current.orientation;
      camera.wrap = current.current.wrap;
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

  // Arrangement follows the measured canvas only while the camera is untouched, so a moved view keeps its geometry.
  const settle = useCallback((whole: boolean, target: string | null) => {
    if (!size) return;
    const plan = planAutomationView(graph, size, whole, target);
    if (!plan.transform) return;
    setArrangement((previous) => previous.orientation === plan.orientation && previous.wrap === plan.wrap ? previous : { orientation: plan.orientation, wrap: plan.wrap });
    setTransform(plan.transform);
    fitted.current = plan.transform;
    moved.current = false;
    overview.current = whole;
    framedFocus.current = target;
  }, [graph, size, setTransform]);
  const fit = useCallback(() => settle(false, focus), [settle, focus]);
  const showOverview = useCallback(() => settle(true, null), [settle]);
  const panelWasOpen = useRef(false);
  useLayoutEffect(() => {
    if (panelOpen && !panelWasOpen.current && size) settle(true, null);
    panelWasOpen.current = panelOpen && size !== null;
  }, [panelOpen, size, settle]);

  // A moved camera keeps its scale and the world point at the viewport center when the canvas resizes.
  const follow = useRef(() => {});
  follow.current = () => {
    if (!size) return;
    const previous = framedSize.current;
    framedSize.current = size;
    if (!userMoved()) return settle(overview.current, framedFocus.current);
    const t = transformRef.current;
    const next = previous ? { ...t, x: t.x + (size.width - previous.width) / 2, y: t.y + (size.height - previous.height) / 2 } : t;
    if (!viewportShowsBounds(next, layouts[current.current.orientation].bounds, size)) return settle(overview.current, framedFocus.current);
    if (!same(next, t)) setTransform(next);
    moved.current = true;
  };
  const { horizontal, vertical } = layouts;
  useLayoutEffect(() => follow.current(), [size?.width, size?.height, horizontal.bounds.width, horizontal.bounds.height, vertical.bounds.width, vertical.bounds.height]);

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

  return { ...viewport, layout: layouts[orientation], measured: size !== null, fit, showOverview, zoom, reveal, onKeyDown, touchHandlers };
}
