import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useCanvasViewport, type ViewportBox } from "@codeterm/canvas-camera";
import { swipeDirection } from "./galleryModel";

export function ZoomCanvas({ bounds, focus, children, onNavigate }: {
  bounds: ViewportBox;
  focus?: ViewportBox | null;
  children: ReactNode;
  onNavigate?: (direction: number) => void;
}) {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const camera = useCanvasViewport({ ...size, minScale: 0.0001, maxScale: 8, wheelBehavior: "zoom", touch: true });
  const { containerRef, layerRef, fitToView, animateToView } = camera;
  const touch = useRef<{ x: number; y: number; time: number; pinched: boolean } | null>(null);
  const lastTap = useRef<{ x: number; y: number; time: number } | null>(null);
  const mouseStart = useRef<{ x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      setSize(previous => previous.width === rect.width && previous.height === rect.height ? previous : { width: rect.width, height: rect.height });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [containerRef]);
  useLayoutEffect(() => { fitToView(bounds); }, [bounds, fitToView]);
  useLayoutEffect(() => { if (focus) animateToView(focus); }, [focus, animateToView]);
  return <>
    <div className="flex shrink-0 gap-2 p-1">
      <button type="button" onClick={() => fitToView(bounds)} aria-label="Show all">Show all</button>
      <button type="button" onClick={() => camera.zoomBy(0.25)} aria-label="Zoom in">+</button>
      <button type="button" onClick={() => camera.zoomBy(-0.2)} aria-label="Zoom out">−</button>
    </div>
    <div ref={containerRef} {...camera.handlers} role="region" aria-label="Zoomable canvas"
      className="relative min-h-0 flex-1 overflow-hidden"
      style={{ touchAction: "none", overscrollBehavior: "none", cursor: camera.isDragging ? "grabbing" : "grab" }}
      onMouseDownCapture={event => { mouseStart.current = { x: event.clientX, y: event.clientY }; }}
      onClickCapture={event => {
        const start = mouseStart.current;
        if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) event.stopPropagation();
        mouseStart.current = null;
      }}
      onDoubleClick={() => fitToView(bounds)}
      onTouchStart={event => {
        if (event.touches.length > 1) { if (touch.current) touch.current.pinched = true; return; }
        const point = event.touches[0];
        touch.current = { x: point.clientX, y: point.clientY, time: event.timeStamp, pinched: false };
      }}
      onTouchEnd={event => {
        const start = touch.current;
        if (!start || event.touches.length) return;
        touch.current = null;
        const point = event.changedTouches[0];
        const dx = point.clientX - start.x, dy = point.clientY - start.y;
        const direction = swipeDirection(dx, dy, event.timeStamp - start.time, start.pinched);
        if (direction) onNavigate?.(direction);
        if (!start.pinched && Math.hypot(dx, dy) < 12) {
          const last = lastTap.current;
          if (last && event.timeStamp - last.time < 320 && Math.hypot(point.clientX - last.x, point.clientY - last.y) < 30) {
            fitToView(bounds); lastTap.current = null;
          } else {
            lastTap.current = { x: point.clientX, y: point.clientY, time: event.timeStamp };
            if (event.target instanceof Element) event.target.closest("button")?.click();
          }
        }
      }}>
      <div ref={layerRef} style={{ position: "absolute", left: 0, top: 0, transformOrigin: "0 0" }}>{children}</div>
    </div>
  </>;
}
