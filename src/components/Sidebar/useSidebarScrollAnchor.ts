import { useCallback, useLayoutEffect, useRef } from "react";
import { minimalScrollDeltaForItem } from "./sidebarSections";

export function useSidebarScrollAnchor(selectedWorkspaceKey: string | null) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const previous = useRef<string | null>(selectedWorkspaceKey);
  useLayoutEffect(() => {
    if (previous.current === selectedWorkspaceKey) return;
    previous.current = selectedWorkspaceKey;
    const host = scroller.current;
    if (!host || !selectedWorkspaceKey) return;
    const activeCard = host.querySelector<HTMLElement>('[data-testid="workspace-card"][data-selected="true"]');
    if (!activeCard) return;
    const delta = minimalScrollDeltaForItem(host.getBoundingClientRect(), activeCard.getBoundingClientRect());
    if (delta === 0) return;
    const current = host.scrollTop;
    const target = Math.max(0, current + delta);
    if (target !== current) host.scrollTop = target;
  }, [selectedWorkspaceKey]);
  return useCallback((host: HTMLDivElement | null) => {
    scroller.current = host;
    if (!host) return;
    return () => {
      scroller.current = null;
    };
  }, []);
}
