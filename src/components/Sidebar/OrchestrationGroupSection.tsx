import type { ReactNode } from "react";
import { SECTION_LABEL, type OrchestrationSection } from "./orchestrationSections";

function sectionTestId(section: OrchestrationSection): string {
  return `group-section-${section}s`;
}

/** A titled band inside an orchestration group (Managers / Workers). Renders
 *  the header ONLY when the section is non-empty, so a worker-less group shows
 *  no "Workers" rule and a solo orchestrator shows no "Managers" rule. */
export function OrchestrationGroupSection({
  section,
  count,
  children,
}: {
  section: OrchestrationSection;
  count: number;
  children: ReactNode;
}) {
  if (count <= 0) return null;
  return (
    <div data-group-section-wrap={section}>
      <div
        data-testid={sectionTestId(section)}
        data-group-section={section}
        className="sidebar-group-section-header px-2 pt-1 pb-0.5 text-[9px] font-medium uppercase tracking-wider text-[var(--ct-muted)]/70 select-none"
      >
        {SECTION_LABEL[section]}
      </div>
      {children}
    </div>
  );
}
