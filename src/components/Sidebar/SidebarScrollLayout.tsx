import { useCallback, useLayoutEffect, useRef, useState, type ReactNode, type RefCallback } from "react";
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
  const keyboardReveal = useRef<SidebarSelectedRow | null>(null);
  useLayoutEffect(() => {
    const row = keyboardReveal.current;
    if (!row) return;
    if (cue?.row !== row) {
      keyboardReveal.current = null;
    } else if (!cue.visible) {
      keyboardReveal.current = null;
      const activation = row.element instanceof HTMLButtonElement ? row.element
        : row.element.querySelector<HTMLButtonElement>(".sidebar-tab-activation");
      activation?.focus({ preventScroll: true });
    }
  }, [cue]);

  if (!cue) return null;
  const content = <>
    <span className="ct-sidebar-selection-cue__dot" aria-hidden="true" />
    {cue.edge === "above" && <span className="ct-sidebar-selection-cue__arrow" aria-hidden="true">↑</span>}
    <span className="ct-sidebar-selection-cue__title">{cue.row.title}</span>
    {cue.edge === "below" && <span className="ct-sidebar-selection-cue__arrow" aria-hidden="true">↓</span>}
  </>;
  return <div className="ct-sidebar-selection-cue" data-edge={cue.edge} data-visible={cue.visible || undefined}
    style={cue.edge === "above" && cue.headerHeight ? { marginTop: cue.headerHeight } : undefined}>
    {cue.visible ? <button type="button" className="ct-sidebar-selection-cue__pill ct-focus-ring"
      aria-label={`Show selected tab, ${cue.row.title}, ${cue.edge}`}
      onBlur={event => {
        if (event.relatedTarget) keyboardReveal.current = null;
      }}
      onClick={event => {
        if (event.detail === 0 && event.currentTarget === document.activeElement) keyboardReveal.current = cue.row;
        cue.row.element.scrollIntoView({ block: "nearest", behavior: reducedMotion ? "auto" : "smooth" });
      }}>
      {content}
    </button> : <span className="ct-sidebar-selection-cue__pill" aria-hidden="true">{content}</span>}
  </div>;
}
