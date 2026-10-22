import { useLayoutEffect, useRef, useState, type HTMLAttributes } from "react";
import { useCanvasViewport, type ViewportBox } from "@codeterm/canvas-camera";
import { swipeDirection } from "./galleryModel";

export interface CanvasFocus { request: number; bounds: ViewportBox }
type ViewMode = "fit" | "focus" | "custom" | "natural";

export function useZoomCanvas({ bounds, focus, wheelBehavior, onNavigate, onSelect }: {
  bounds: ViewportBox;
  focus?: CanvasFocus | null;
  wheelBehavior: "pan" | "zoom";
  onNavigate?: (direction: number) => void;
  onSelect?: (index: number) => void;
}) {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [mode, setMode] = useState<ViewMode>("fit");
  const modeRef = useRef<ViewMode>("fit");
  const focusRequest = useRef<number | null>(null);
  const scaleOutput = useRef<HTMLOutputElement>(null);
  const camera = useCanvasViewport({ ...size, minScale: 0.0001, maxScale: 8, wheelBehavior, touch: true, constrainToFit: false,
    onPaint: transform => { if (scaleOutput.current) scaleOutput.current.textContent = `${Math.round(transform.scale * 100)}%`; },
  });
  const { containerRef, fitToView, animateToView } = camera;
  const touch = useRef<{ x: number; y: number; time: number; pinched: boolean } | null>(null);
  const lastTap = useRef<{ x: number; y: number; time: number } | null>(null);
  const mouseStart = useRef<{ x: number; y: number } | null>(null);
  const handledTap = useRef<{ x: number; y: number; time: number } | null>(null);
  const changeMode = (next: ViewMode) => { modeRef.current = next; setMode(next); };
  const fit = () => { changeMode("fit"); fitToView(bounds); };
  const zoom = (delta: number) => { changeMode("custom"); camera.zoomBy(delta); };
  const natural = () => {
    changeMode("natural");
    camera.setTransform({ x: size.width / 2 - bounds.x - bounds.width / 2, y: size.height / 2 - bounds.y - bounds.height / 2, scale: 1 });
  };
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      setSize(previous => previous.width === rect.width && previous.height === rect.height ? previous : { width: rect.width, height: rect.height });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [containerRef]);
  useLayoutEffect(() => {
    if (focus && focus.request !== focusRequest.current) {
      focusRequest.current = focus.request;
      modeRef.current = "focus";
      setMode("focus");
    }
    if (modeRef.current === "fit") fitToView(bounds);
    else if (modeRef.current === "focus" && focus) animateToView(focus.bounds);
  }, [bounds, focus, fitToView, animateToView]);
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
    onDoubleClick: fit,
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
        if (last && event.timeStamp - last.time < 320 && Math.hypot(point.clientX - last.x, point.clientY - last.y) < 30) {
          fit(); lastTap.current = null;
        } else {
          lastTap.current = { x: point.clientX, y: point.clientY, time: event.timeStamp };
          const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-zoom-target]") : null;
          if (target && target.dataset.zoomTarget !== undefined) onSelect?.(Number(target.dataset.zoomTarget));
        }
      }
    },
    onKeyDown: event => {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      if (["Equal", "NumpadAdd"].includes(event.code)) zoom(0.25);
      else if (["Minus", "NumpadSubtract"].includes(event.code)) zoom(-0.2);
      else if (event.code === "Digit0") natural();
      else if (event.code === "Home") fit();
      else if (onNavigate && ["ArrowRight", "ArrowLeft"].includes(event.code)) onNavigate(event.code === "ArrowRight" ? 1 : -1);
      else if (event.code.startsWith("Arrow")) {
        changeMode("custom");
        const t = camera.transformRef.current;
        camera.setTransform({ ...t, x: t.x + (event.code === "ArrowLeft" ? 40 : event.code === "ArrowRight" ? -40 : 0), y: t.y + (event.code === "ArrowUp" ? 40 : event.code === "ArrowDown" ? -40 : 0) });
      } else return;
      event.preventDefault(); event.stopPropagation();
    },
  };
  return { camera, mode, scaleOutput, interactions, fit, zoom, natural };
}
