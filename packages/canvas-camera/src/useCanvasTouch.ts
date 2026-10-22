import { useEffect, type RefObject } from "react";
import { touchTransform, type CameraPoint } from "./touchModel";
import type { ViewportTransform } from "./viewportModel";

export function useCanvasTouch(containerRef: RefObject<HTMLDivElement | null>, transformRef: RefObject<ViewportTransform>, touch: boolean, cancelAnim: () => void, schedule: (next: ViewportTransform) => void, zoomFloor: () => number, maxScale: number) {
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !touch) return;
    let previous: { points: CameraPoint[]; transform: ViewportTransform } | null = null;
    const points = (event: TouchEvent) => {
      const rect = el.getBoundingClientRect();
      return Array.from(event.touches).slice(0, 2).map(t => ({ x: t.clientX - rect.left, y: t.clientY - rect.top }));
    };
    const start = (event: TouchEvent) => {
      event.preventDefault();
      cancelAnim();
      previous = { points: points(event), transform: transformRef.current };
    };
    const move = (event: TouchEvent) => {
      event.preventDefault();
      const next = points(event);
      if (previous && previous.points.length === next.length && next.length) {
        schedule(touchTransform(previous.transform, previous.points, next, zoomFloor(), maxScale));
      } else previous = { points: next, transform: transformRef.current };
    };
    el.addEventListener("touchstart", start, { passive: false });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", start, { passive: false });
    el.addEventListener("touchcancel", start, { passive: false });
    return () => {
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", start);
      el.removeEventListener("touchcancel", start);
    };
  }, [containerRef, transformRef, touch, cancelAnim, schedule, zoomFloor, maxScale]);

}
