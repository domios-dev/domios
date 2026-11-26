import { useLayoutEffect, useRef } from "react";
import { PlanStatusBreakdown, type PlanStatusBreakdownProps } from "../shared/PlanStatusBreakdown";
import { compactTeamStatus } from "./teamStatusFit";

export function TeamStatusTally(props: PlanStatusBreakdownProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  // A ResizeObserver first reports after layout and before paint, so the fit lands in the first frame
  // without this commit forcing a layout of the whole list it just inserted.
  useLayoutEffect(() => {
    const container = containerRef.current;
    const summary = container?.firstElementChild as HTMLElement | null;
    if (!container || !summary) return;
    let cachedFullWidth: number | null = null;
    const measure = () => {
      const availableWidth = container.getBoundingClientRect().width;
      if (cachedFullWidth === null) {
        if (container.hasAttribute("data-compact")) container.removeAttribute("data-compact");
        cachedFullWidth = summary.getBoundingClientRect().width;
      }
      const compact = compactTeamStatus(cachedFullWidth, availableWidth);
      if (container.hasAttribute("data-compact") !== compact) {
        container.toggleAttribute("data-compact", compact);
      }
    };
    const onFontsLoaded = () => {
      cachedFullWidth = null;
      measure();
    };
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    if (observer) observer.observe(container);
    else measure();
    document.fonts?.addEventListener("loadingdone", onFontsLoaded);
    return () => {
      observer?.disconnect();
      document.fonts?.removeEventListener("loadingdone", onFontsLoaded);
    };
  }, [props.counts]);

  return <div ref={containerRef} className="sidebar-team-status-fit"><PlanStatusBreakdown {...props} /></div>;
}
