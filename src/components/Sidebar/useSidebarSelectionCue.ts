import { useLayoutEffect, useState } from "react";
import type { SidebarSelectedRow } from "./sidebarSelectedRow";

interface SelectionCue {
  row: SidebarSelectedRow;
  edge: "above" | "below";
  visible: boolean;
}

export function useSidebarSelectionCue(
  root: HTMLElement | null, tabId: string | null, registeredRows: readonly SidebarSelectedRow[],
) {
  const [cue, setCue] = useState<SelectionCue | null>(null);

  useLayoutEffect(() => {
    setCue(null);
    if (!root || !tabId || typeof IntersectionObserver === "undefined") return;
    const rows = registeredRows.filter(row => row.tabId === tabId && root.contains(row.element));
    if (!rows.length) return;

    let selected: SidebarSelectedRow | undefined;
    let oppositeEdge: { edge: SelectionCue["edge"]; scrollTop: number } | undefined;
    // The inset makes edge contact mean one pixel of the actual viewport is visible.
    const observer = new IntersectionObserver((entries) => {
      const row = selected;
      const entry = entries.find(candidate => candidate.target === row?.element);
      if (!row || !entry?.rootBounds) return;
      oppositeEdge = undefined;
      if (entry.boundingClientRect.height === 0) {
        setCue(null);
      } else if (entry.isIntersecting) {
        setCue(previous => previous?.visible ? { ...previous, visible: false } : previous);
      } else {
        const edge = entry.boundingClientRect.top < entry.rootBounds.top ? "above" : "below";
        const distance = edge === "above"
          ? entry.boundingClientRect.top - entry.rootBounds.bottom
          : entry.boundingClientRect.bottom - entry.rootBounds.top;
        oppositeEdge = { edge, scrollTop: root.scrollTop + distance };
        setCue(previous => previous?.row === row && previous.edge === edge && previous.visible
          ? previous : { row, edge, visible: true });
      }
    }, { root, rootMargin: "-1px 0px", threshold: [0, 1] });

    const observeVisibleRow = () => {
      const next = rows.find(({ element }) => !element.closest("[hidden]"));
      if (next === selected) return;
      oppositeEdge = undefined;
      if (selected) {
        observer.unobserve(selected.element);
        observer.takeRecords();
      }
      selected = next;
      setCue(null);
      if (selected) observer.observe(selected.element);
    };
    const resampleAfterJump = () => {
      if (!selected || !oppositeEdge) return;
      const top = root.scrollTop;
      const skippedViewport = oppositeEdge.edge === "above" ? top < oppositeEdge.scrollTop : top > oppositeEdge.scrollTop;
      if (!skippedViewport) return;
      // A jump across both edges can leave the observer's intersection state unchanged.
      oppositeEdge = undefined;
      observer.unobserve(selected.element);
      observer.takeRecords();
      observer.observe(selected.element);
    };
    const visibility = new MutationObserver(observeVisibleRow);
    for (const { element } of rows) {
      for (let ancestor: HTMLElement | null = element; ancestor && ancestor !== root; ancestor = ancestor.parentElement) {
        visibility.observe(ancestor, { attributes: true, attributeFilter: ["hidden"] });
      }
    }
    observeVisibleRow();
    root.addEventListener("scroll", resampleAfterJump, { passive: true });
    return () => {
      selected = undefined;
      root.removeEventListener("scroll", resampleAfterJump);
      visibility.disconnect();
      observer.disconnect();
    };
  }, [root, tabId, registeredRows]);

  return cue;
}
