import { useEffect, useRef, type RefObject } from "react";
import { CANVAS_ZOOM_INTENSITY, type ViewportTransform } from "./viewportModel";

function pixelDelta(value: number, mode: number): number {
  return mode === 1 ? value * 16 : mode === 2 ? value * 384 : value;
}

export function useCanvasWheel(containerRef: RefObject<HTMLDivElement | null>, transformRef: RefObject<ViewportTransform>, zoomAround: (factor: number, x: number, y: number) => void, schedule: (next: ViewportTransform) => void, wheelBehavior: "pan" | "zoom") {
  const boundaryArmRef = useRef<{ el: HTMLElement; released: boolean } | null>(null);
  const lastWheelTsRef = useRef(0);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const rect = el.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey || wheelBehavior === "zoom") {
        e.preventDefault();
        const dy = pixelDelta(e.deltaY, e.deltaMode);
        zoomAround(Math.exp(-dy * CANVAS_ZOOM_INTENSITY), e.clientX - rect.left, e.clientY - rect.top);
        return;
      }
      const dx = pixelDelta(e.deltaX, e.deltaMode);
      const dy = pixelDelta(e.deltaY, e.deltaMode);
      const gap = e.timeStamp - lastWheelTsRef.current;
      lastWheelTsRef.current = e.timeStamp;
      const newGesture = gap > 220; // a deliberate re-scroll, not a momentum tail
      const body = (e.target as HTMLElement | null)?.closest?.("[data-canvas-scroll-body]") as HTMLElement | null;
      const sideways = Math.abs(dx) > Math.abs(dy);
      if (body && !sideways) {
        const canScroll = body.scrollHeight - body.clientHeight > 1;
        const dir = Math.sign(dy);
        const atBoundary =
          (dir < 0 && body.scrollTop <= 0) ||
          (dir > 0 && body.scrollTop + body.clientHeight >= body.scrollHeight - 1);
        if (canScroll && !atBoundary) {
          boundaryArmRef.current = null; // scrolling within content → disarm
          return; // node native-scrolls; no pan
        }
        if (canScroll && atBoundary) {
          const arm = boundaryArmRef.current;
          if (arm?.el === body && arm.released) {
            // already broke free → keep panning the canvas (no stutter / re-arm)
          } else if (arm?.el === body && newGesture) {
            boundaryArmRef.current = { el: body, released: true }; // fresh scroll → release
          } else {
            boundaryArmRef.current = { el: body, released: false }; // rubber-band beat
            e.preventDefault();
            return;
          }
        }
      } else {
        boundaryArmRef.current = null;
      }
      e.preventDefault();
      const t = transformRef.current;
      schedule({ ...t, x: t.x - dx, y: t.y - dy });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [containerRef, zoomAround, schedule, wheelBehavior]);

}
