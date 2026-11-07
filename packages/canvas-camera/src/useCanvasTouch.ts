import { useEffect, type RefObject } from "react";
import { touchTransform, type CameraPoint } from "./touchModel";
import type { ViewportTransform } from "./viewportModel";

/** Controls float over the canvas; a touch on them is a tap, never the start of a pan. */
const CONTROLS = "[data-graph-toolbar]";

export function useCanvasTouch(containerRef: RefObject<HTMLDivElement | null>, transformRef: RefObject<ViewportTransform>, touch: boolean, cancelAnim: () => void, schedule: (next: ViewportTransform) => void, zoomFloor: () => number, maxScale: number) {
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !touch) return;
    let previous: { points: CameraPoint[]; transform: ViewportTransform } | null = null;
    const points = (event: TouchEvent) => {
      const rect = el.getBoundingClientRect();
      return Array.from(event.touches).slice(0, 2).map(t => ({ x: t.clientX - rect.left, y: t.clientY - rect.top }));
    };
    const onControls = (event: TouchEvent) => event.target instanceof Element && event.target.closest(CONTROLS) !== null;
    // Starting or lifting a finger re-anchors the gesture. One finger is never cancelled, because buttons
    // and cards inside the canvas still need its tap; a second finger is a pinch, kept from the page at once.
    const anchor = (event: TouchEvent) => {
      if (onControls(event)) { previous = null; return; }
      if (event.type === "touchstart" && event.touches.length > 1) event.preventDefault();
      cancelAnim();
      previous = event.touches.length ? { points: points(event), transform: transformRef.current } : null;
    };
    // Moving is the gesture: the canvas pans and pinches, and the page neither scrolls nor zooms.
    const move = (event: TouchEvent) => {
      if (!previous) return;
      event.preventDefault();
      const next = points(event);
      if (previous.points.length === next.length && next.length) {
        schedule(touchTransform(previous.transform, previous.points, next, zoomFloor(), maxScale));
      } else previous = { points: next, transform: transformRef.current };
    };
    // Safari zooms the page on its own gesture events, which touch-action alone does not stop.
    const gesture = (event: Event) => event.preventDefault();
    el.addEventListener("touchstart", anchor, { passive: false });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", anchor, { passive: true });
    el.addEventListener("touchcancel", anchor, { passive: true });
    el.addEventListener("gesturestart", gesture, { passive: false });
    el.addEventListener("gesturechange", gesture, { passive: false });
    return () => {
      el.removeEventListener("touchstart", anchor);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", anchor);
      el.removeEventListener("touchcancel", anchor);
      el.removeEventListener("gesturestart", gesture);
      el.removeEventListener("gesturechange", gesture);
    };
  }, [containerRef, transformRef, touch, cancelAnim, schedule, zoomFloor, maxScale]);

}
