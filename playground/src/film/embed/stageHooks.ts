import { useEffect, useState } from "react";

/** The factor that fits a stage `width` wide into the box. */
export function useScale(box: HTMLElement | null, width: number) {
  const [scale, setScale] = useState(0);
  useEffect(() => {
    if (!box) return;
    const observer = new ResizeObserver(() => setScale(box.clientWidth / width));
    observer.observe(box);
    return () => observer.disconnect();
  }, [box, width]);
  return scale;
}

/**
 * Keeps wheel events from the product's canvases, which pan or zoom on wheel and cancel it, so the page always scrolls
 * over the film. Native scrolling of an inner scroll area is a default action and still happens.
 */
export function useWheelPassThrough(box: HTMLElement | null) {
  useEffect(() => {
    if (!box) return;
    const stop = (e: WheelEvent) => e.stopPropagation();
    box.addEventListener("wheel", stop, { capture: true, passive: true });
    return () => box.removeEventListener("wheel", stop, { capture: true });
  }, [box]);
}
