import { useLayoutEffect, useRef, useState } from "react";

export function useCompactToolRow() {
  const header = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(false);
  useLayoutEffect(() => {
    const element = header.current;
    if (!element) return;
    const column = element.closest(".ct-chat-surface") ?? element;
    const coarse = window.matchMedia("(pointer: coarse)");
    const measure = () => {
      const width = column.getBoundingClientRect().width;
      setCompact(coarse.matches || (width > 0 && width < 480));
    };
    measure();
    coarse.addEventListener("change", measure);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(column);
    return () => {
      coarse.removeEventListener("change", measure);
      observer?.disconnect();
    };
  }, []);
  return { header, compact };
}
