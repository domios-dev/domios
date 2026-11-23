import { useCallback, useState, type ReactNode, type RefCallback } from "react";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";
import { SidebarSelectedRowContext, type SidebarSelectedRow } from "./sidebarSelectedRow";
import { useSidebarSelectionCue } from "./useSidebarSelectionCue";

interface SidebarScrollLayoutProps {
  activeTabId: string | null;
  children: (scrollRef: RefCallback<HTMLDivElement> | undefined) => ReactNode;
  scrollRef: RefCallback<HTMLDivElement>;
}

export function SidebarScrollLayout({ activeTabId, children, scrollRef }: SidebarScrollLayoutProps) {
  const phone = usePhoneViewport();
  const [root, setRoot] = useState<HTMLDivElement | null>(null);
  const [rows, setRows] = useState<SidebarSelectedRow[]>([]);
  const registerRow = useCallback((row: SidebarSelectedRow) => {
    setRows(previous => [...previous, row]);
    return () => setRows(previous => previous.filter(candidate => candidate !== row));
  }, []);
  const attachScrollRoot = useCallback((element: HTMLDivElement | null) => {
    setRoot(element);
    const detach = scrollRef(element);
    if (!element) return;
    return () => {
      setRoot(null);
      if (typeof detach === "function") detach();
    };
  }, [scrollRef]);
  return <>
    <div ref={phone ? attachScrollRoot : undefined} className="ct-sidebar-scroll-shell">
      <SidebarSelectedRowContext.Provider value={registerRow}>
        {children(phone ? undefined : attachScrollRoot)}
      </SidebarSelectedRowContext.Provider>
    </div>
    <SidebarSelectionCue root={root} activeTabId={activeTabId} rows={rows} />
  </>;
}

function SidebarSelectionCue({ root, activeTabId, rows }: {
  root: HTMLElement | null; activeTabId: string | null; rows: readonly SidebarSelectedRow[];
}) {
  const cue = useSidebarSelectionCue(root, activeTabId, rows);
  const reducedMotion = usePrefersReducedMotion();
  if (!cue) return null;
  return <div className="ct-sidebar-selection-cue" data-edge={cue.edge} data-visible={cue.visible || undefined}
    aria-hidden={!cue.visible || undefined} inert={!cue.visible || undefined}>
    <button type="button" className="ct-sidebar-selection-cue__button ct-focus-ring"
      aria-label={`Show selected tab, ${cue.row.title}, ${cue.edge}`}
      onClick={() => cue.row.element.scrollIntoView({ block: "nearest", behavior: reducedMotion ? "auto" : "smooth" })}>
      <span className="ct-sidebar-selection-cue__dot" aria-hidden="true" />
      {cue.edge === "above" && <span className="ct-sidebar-selection-cue__arrow" aria-hidden="true">↑</span>}
      <span className="ct-sidebar-selection-cue__title">{cue.row.title}</span>
      {cue.edge === "below" && <span className="ct-sidebar-selection-cue__arrow" aria-hidden="true">↓</span>}
    </button>
  </div>;
}
