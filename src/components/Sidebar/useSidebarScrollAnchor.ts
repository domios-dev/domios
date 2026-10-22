import { useLayoutEffect, useRef } from "react";
import { retainSidebarScrollAnchor } from "./sidebarScrollAnchor";

export function useSidebarScrollAnchor(active: boolean) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const spaceRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!active || !viewportRef.current || !bodyRef.current || !spaceRef.current) return;
    return retainSidebarScrollAnchor(viewportRef.current, bodyRef.current, spaceRef.current);
  }, [active]);
  return { viewportRef, bodyRef, spaceRef };
}
