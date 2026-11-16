import { useLayoutEffect, useRef, useState, type HTMLAttributes, type RefObject } from "react";
import { useCanvasViewport, type UseCanvasViewportOptions, type UseCanvasViewportResult, type ViewportTransform } from "@codeterm/canvas-camera";

import { createCanvasStageMemory, type CanvasStageMemory } from "./canvasStageMemory";

export const CANVAS_ZOOM_STEP = 0.25;
export interface CanvasStageSize { width: number; height: number }

interface CanvasStageOptions extends Omit<UseCanvasViewportOptions, "width" | "height" | "minScale"> {
  minScale?: number | ((size: CanvasStageSize) => number);
  measureKey?: string;
  initialSize?: CanvasStageSize;
  measureRef?: RefObject<HTMLDivElement | null>;
  contextKey?: string;
  memory?: CanvasStageMemory;
  memoryKey?: string;
  ready?: boolean;
  cancelled?: boolean;
  fit: (camera: UseCanvasViewportResult, size: CanvasStageSize, initial: boolean) => boolean | void;
  followFit?: boolean;
  fitRevision?: string;
  onUserControl?: () => void;
  onReset?: () => void;
  onNavigate?: (direction: number) => void;
}

export function useCanvasStage(options: CanvasStageOptions) {
  const [size, setSize] = useState(options.initialSize ?? { width: 0, height: 0 });
  const measuredSize = useRef(options.initialSize ?? { width: 0, height: 0 });
  const measured = useRef(!!options.initialSize);
  const state = useRef({ key: options.contextKey, initialized: false, moved: false, revision: "" });
  const [memory] = useState(() => createCanvasStageMemory(options.memoryKey, options.memory));
  const restored = useRef(!!memory.read()?.transform);
  const latest = useRef(options);
  latest.current = options;
  const remember = (transform: ViewportTransform) => {
    if (state.current.initialized) memory.remember(transform, state.current.moved);
  };
  const camera = useCanvasViewport({ ...options, ...size, minScale: typeof options.minScale === "function" ? options.minScale(size) : options.minScale, touch: options.touch ?? true, onPaint: transform => {
    remember(transform);
    latest.current.onPaint?.(transform);
  } });
  const cameraRef = useRef(camera);
  cameraRef.current = camera;
  const takeControl = () => {
    latest.current.onUserControl?.();
    state.current.initialized = true;
    state.current.moved = true;
    remember(camera.transformRef.current);
  };
  const fit = (initial = false) => {
    if (latest.current.fit(cameraRef.current, size, initial) === false) return;
    state.current.initialized = true;
    state.current.moved = false;
    remember(cameraRef.current.transformRef.current);
  };
  useLayoutEffect(() => {
    const element = options.measureRef?.current ?? camera.containerRef.current;
    if (!element) return;
    const measure = (entries?: ResizeObserverEntry[]) => {
      const rect = entries?.[0]?.contentRect ?? element.getBoundingClientRect();
      const width = entries?.[0]?.contentRect.width ?? (element.clientWidth || rect.width), height = entries?.[0]?.contentRect.height ?? (element.clientHeight || rect.height);
      if (width <= 0 || height <= 0) return;
      measuredSize.current = { width, height };
      measured.current = true;
      setSize(previous => previous.width === width && previous.height === height ? previous : { width, height });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [options.measureRef, options.measureKey, camera.containerRef]);
  useLayoutEffect(() => {
    const current = state.current;
    if (current.key !== options.contextKey) {
      current.key = options.contextKey;
      current.initialized = false;
      current.moved = false;
    }
    if (options.cancelled) {
      current.initialized = true;
      current.moved = true;
      remember(camera.transformRef.current);
    }
    if (!measured.current || size.width <= 0 || size.height <= 0 || measuredSize.current.width !== size.width || measuredSize.current.height !== size.height) return;
    if (!current.initialized && memory.read()?.transform) {
      current.initialized = true;
      current.moved = memory.read()!.userMoved;
      camera.setTransform(memory.read()!.transform!);
    }
    const revision = `${size.width}:${size.height}:${options.fitRevision ?? ""}`;
    if (options.ready !== false && (!current.initialized || options.followFit && !current.moved && current.revision !== revision)) fit(!current.initialized);
    current.revision = revision;
  });
  const zoom = (delta: number) => { takeControl(); camera.zoomBy(delta); };
  const onKeyDown: NonNullable<HTMLAttributes<HTMLDivElement>["onKeyDown"]> = event => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.target !== event.currentTarget) return;
    if (["Equal", "NumpadAdd"].includes(event.code)) zoom(CANVAS_ZOOM_STEP);
    else if (["Minus", "NumpadSubtract"].includes(event.code)) zoom(-CANVAS_ZOOM_STEP);
    else if (["Digit0", "Numpad0"].includes(event.code)) { if (options.onReset) options.onReset(); else fit(); }
    else if (event.code === "Home") fit();
    else if (options.onNavigate && ["ArrowLeft", "ArrowRight"].includes(event.code)) options.onNavigate(event.code === "ArrowRight" ? 1 : -1);
    else if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.code)) {
      takeControl();
      const t = camera.transformRef.current;
      camera.setTransform({ ...t, x: t.x + (event.code === "ArrowLeft" ? 40 : event.code === "ArrowRight" ? -40 : 0), y: t.y + (event.code === "ArrowUp" ? 40 : event.code === "ArrowDown" ? -40 : 0) });
    } else return;
    event.preventDefault(); event.stopPropagation();
  };
  const onInteraction: NonNullable<HTMLAttributes<HTMLDivElement>["onMouseDownCapture"]> = event => {
    if (event.target instanceof Element && event.target.closest("[data-graph-toolbar], button, input, textarea, select")) return;
    takeControl();
  };
  const interactions: HTMLAttributes<HTMLDivElement> = {
    onMouseDownCapture: onInteraction, onWheelCapture: takeControl, onTouchStartCapture: takeControl, onKeyDown,
  };
  return { camera, size, restored: restored.current, measured: measured.current, fit, zoom, takeControl, interactions, userMoved: () => state.current.moved };
}
