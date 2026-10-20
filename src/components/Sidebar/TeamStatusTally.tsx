import { useLayoutEffect, useRef } from "react";
import { PlanStatusBreakdown, type PlanStatusBreakdownProps } from "../shared/PlanStatusBreakdown";
import { compactTeamStatus } from "./teamStatusFit";

export function TeamStatusTally(props: PlanStatusBreakdownProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const container = containerRef.current;
    const summary = container?.firstElementChild as HTMLElement | null;
    if (!container || !summary) return;
    let active = true;
    const measure = () => {
      if (!active) return;
      container.removeAttribute("data-compact");
      const compact = compactTeamStatus(summary.getBoundingClientRect().width, container.getBoundingClientRect().width);
      container.toggleAttribute("data-compact", compact);
    };
    measure();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(container);
    document.fonts?.addEventListener("loadingdone", measure);
    void document.fonts?.ready.then(measure);
    return () => {
      active = false;
      observer?.disconnect();
      document.fonts?.removeEventListener("loadingdone", measure);
    };
  }, [props.counts]);

  return <div ref={containerRef} className="sidebar-team-status-fit"><PlanStatusBreakdown {...props} /></div>;
}
