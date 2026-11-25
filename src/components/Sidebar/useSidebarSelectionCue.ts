import { useLayoutEffect, useState } from "react";
import type { SidebarSelectedRow } from "./sidebarSelectedRow";

interface SelectionCue {
  row: SidebarSelectedRow;
  edge: "above" | "below";
  headerHeight: number;
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
    let observer: IntersectionObserver | undefined;
    let header: HTMLElement | null = null;
    let headerHeight = 0;
    const observeRow = () => {
      const row = selected;
      observer?.disconnect();
      if (!row) return;
      if (headerHeight) row.element.style.setProperty("--ct-sidebar-sticky-header", `${headerHeight}px`);
      else row.element.style.removeProperty("--ct-sidebar-sticky-header");
      const height = headerHeight;
      // The one-pixel inset keeps edge contact meaningful under the sticky header.
      observer = new IntersectionObserver((entries, currentObserver) => {
        if (currentObserver !== observer || row !== selected) return;
        const entry = entries.find(candidate => candidate.target === row.element);
        if (!entry?.rootBounds) return;
        oppositeEdge = undefined;
        if (entry.boundingClientRect.height === 0 || entry.rootBounds.height === 0) {
          setCue(null);
        } else if (entry.isIntersecting) {
          setCue(previous => previous?.visible ? { ...previous, visible: false } : previous);
        } else {
          const edge = entry.boundingClientRect.top < entry.rootBounds.top ? "above" : "below";
          const distance = edge === "above"
            ? entry.boundingClientRect.top - entry.rootBounds.bottom
            : entry.boundingClientRect.bottom - entry.rootBounds.top;
          oppositeEdge = { edge, scrollTop: root.scrollTop + distance };
          setCue(previous => previous?.row === row && previous.edge === edge && previous.headerHeight === height && previous.visible
            ? previous : { row, edge, headerHeight: height, visible: true });
        }
      }, { root, rootMargin: `-${height + 1}px 0px -1px`, threshold: [0, 1] });
      observer.observe(row.element);
    };
    const headerSize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      const height = header?.offsetHeight ?? 0;
      if (height === headerHeight) return;
      headerHeight = height;
      observeRow();
    });

    const observeVisibleRow = () => {
      const next = rows.find(({ element }) => !element.closest("[hidden]"));
      if (next === selected) return;
      oppositeEdge = undefined;
      selected?.element.style.removeProperty("--ct-sidebar-sticky-header");
      headerSize?.disconnect();
      selected = next;
      header = next?.element.closest(".sidebar-workspace-detail")
        ?.querySelector<HTMLElement>(":scope > .sidebar-workspace-detail-header") ?? null;
      headerHeight = header?.offsetHeight ?? 0;
      if (header) headerSize?.observe(header);
      setCue(null);
      observeRow();
    };
    const resampleAfterJump = () => {
      if (!selected || !observer || !oppositeEdge) return;
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
      selected?.element.style.removeProperty("--ct-sidebar-sticky-header");
      selected = undefined;
      root.removeEventListener("scroll", resampleAfterJump);
      headerSize?.disconnect();
      visibility.disconnect();
      observer?.disconnect();
    };
  }, [root, tabId, registeredRows]);

  return cue;
}
