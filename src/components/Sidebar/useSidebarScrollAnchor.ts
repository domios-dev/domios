import { useLayoutEffect, useRef } from "react";
import { sidebarWorkspaceScrollTop, type SidebarScrollScope } from "./sidebarSections";

export function useSidebarScrollAnchor(activeWorkspaceId: string | null, selectedWorkspaceId: string | null) {
  const scroller = useRef<HTMLDivElement>(null);
  const previous = useRef<SidebarScrollScope>({ activeWorkspaceId, selectedWorkspaceId });
  useLayoutEffect(() => {
    const next = { activeWorkspaceId, selectedWorkspaceId };
    const host = scroller.current;
    if (host) host.scrollTop = sidebarWorkspaceScrollTop(previous.current, next, host.scrollTop);
    previous.current = next;
  }, [activeWorkspaceId, selectedWorkspaceId]);
  return scroller;
}
