import { createContext, createElement, useContext, useLayoutEffect, useState, type ReactNode, type RefObject } from "react";
import { useFullscreenViewer } from "./FullscreenOverlay";

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

/**
 * The one layout rule for chat boxes on a phone (coarse pointer or a column under 480 px).
 * `compact`: secondary actions such as full screen live under the card's ⋮, and the row itself
 * is the disclosure, so no separate chevron. `brief`: the row shows its title and one key fact;
 * the full-screen reader is where the long facts come back.
 */
export function useCompactCard(): { compact: boolean; brief: boolean } {
  const compact = useCompactToolRow();
  const inReader = Boolean(useFullscreenViewer());
  return { compact, brief: compact && !inReader };
}
