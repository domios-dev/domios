import { useCallback, useLayoutEffect, useRef } from "react";
import { sidebarWorkspaceScrollTop, type SidebarScrollScope } from "./sidebarSections";

/** Touching scrollTop inside the switch commit forces a layout of the whole new list, so the position is tracked from scroll events. */
export function useSidebarScrollAnchor(activeWorkspaceId: string | null, selectedWorkspaceId: string | null) {
  const scroller = useRef<HTMLDivElement | null>(null);
  const top = useRef(0);
  const previous = useRef<SidebarScrollScope>({ activeWorkspaceId, selectedWorkspaceId });
  useLayoutEffect(() => {
    const next = { activeWorkspaceId, selectedWorkspaceId };
    const target = sidebarWorkspaceScrollTop(previous.current, next, top.current);
    if (scroller.current && target !== top.current) {
      scroller.current.scrollTop = target;
      top.current = target;
    }
    previous.current = next;
  }, [activeWorkspaceId, selectedWorkspaceId]);
  return useCallback((host: HTMLDivElement | null) => {
    scroller.current = host;
    top.current = 0;
    if (!host) return;
    const track = () => { top.current = host.scrollTop; };
    host.addEventListener("scroll", track, { passive: true });
    return () => {
      host.removeEventListener("scroll", track);
      scroller.current = null;
    };
  }, []);
}
