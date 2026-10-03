import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useCanvasViewport } from "../Canvas/useCanvasViewport";
import type { ViewportBox } from "../Canvas/useCanvasViewport";

export function useAutomationViewport(bounds: ViewportBox, layoutKey: string) {
  const [size, setSize] = useState({ width: 720, height: 440 });
  const viewport = useCanvasViewport({ ...size, ignoreSelector: "button", wheelBehavior: "zoom" });
  const { containerRef, fitToView } = viewport;
  const fitted = useRef<string | null>(null);
  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const update = () => {
      if (element.clientWidth && element.clientHeight) setSize({ width: element.clientWidth, height: element.clientHeight });
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [containerRef]);
  useEffect(() => {
    if (fitted.current === layoutKey) return;
    fitted.current = layoutKey;
    fitToView(bounds);
  }, [bounds, layoutKey, fitToView]);
  return viewport;
}
