import { useLayoutEffect, useRef } from "react";

export function useAnimatedHeight() {
  const frameRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const frame = frameRef.current;
    const content = contentRef.current;
    if (!frame || !content || typeof ResizeObserver === "undefined") return;
    const sync = () => {
      frame.style.height = `${content.offsetHeight}px`;
    };
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  return { frameRef, contentRef };
}
