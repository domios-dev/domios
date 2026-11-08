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
    const measure = () => {
      container.removeAttribute("data-compact");
      const compact = compactTeamStatus(summary.getBoundingClientRect().width, container.getBoundingClientRect().width);
      container.toggleAttribute("data-compact", compact);
    };
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    if (observer) observer.observe(container);
    else measure();
    document.fonts?.addEventListener("loadingdone", measure);
    return () => {
      observer?.disconnect();
      document.fonts?.removeEventListener("loadingdone", measure);
    };
  }, [props.counts]);

  return <div ref={containerRef} className="sidebar-team-status-fit"><PlanStatusBreakdown {...props} /></div>;
}
