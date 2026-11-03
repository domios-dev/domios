import { createContext, createElement, useContext, useLayoutEffect, useState, type ReactNode, type RefObject } from "react";

const CompactToolRowsContext = createContext(false);

export function CompactToolRowsProvider({ surfaceRef, children }: { surfaceRef: RefObject<HTMLElement | null>; children: ReactNode }) {
  const [compact, setCompact] = useState(false);
  useLayoutEffect(() => {
    const column = surfaceRef.current;
    if (!column) return;
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
  }, [surfaceRef]);
  return createElement(CompactToolRowsContext.Provider, { value: compact }, children);
}

export function useCompactToolRow() {
  return useContext(CompactToolRowsContext);
}
