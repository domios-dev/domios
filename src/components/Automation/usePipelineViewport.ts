import { useLayoutEffect, useState } from "react";
import { useCanvasViewport } from "@codeterm/canvas-camera";

export function usePipelineViewport(width: number, height: number) {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const viewport = useCanvasViewport({ ...size, ignoreSelector: "button", wheelBehavior: "zoom", touch: true });
  const { containerRef, fitToView } = viewport;
  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const measure = () => setSize((previous) => previous.width === element.clientWidth && previous.height === element.clientHeight
      ? previous : { width: element.clientWidth, height: element.clientHeight });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [containerRef, width, height]);
  const fit = () => fitToView({ x: 0, y: 0, width, height });
  useLayoutEffect(() => { fitToView({ x: 0, y: 0, width, height }); }, [fitToView, width, height]);
  return { ...viewport, fit };
}
