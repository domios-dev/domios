import type { ReactNode } from "react";
import { SECTION_LABEL, type OrchestrationSection } from "./orchestrationSections";

function sectionTestId(section: OrchestrationSection): string {
  return `group-section-${section}s`;
}

/** A band inside an orchestration group (Managers / Workers). Renders the
 *  children whenever the section is non-empty, but the HEADER row only when it
 *  disambiguates (`showHeader`) — a one-planner group shows the planner row
 *  bare, no MANAGERS label. Header math lives in `shouldShowSectionHeader`. */
export function OrchestrationGroupSection({
  section,
  count,
  showHeader = true,
  children,
}: {
  section: OrchestrationSection;
  count: number;
  showHeader?: boolean;
  children: ReactNode;
}) {
  if (count <= 0) return null;
  return (
    <div data-group-section-wrap={section}>
      {showHeader && (
        <div
          data-testid={sectionTestId(section)}
          data-group-section={section}
          className="sidebar-group-section-header pt-1.5 pb-1 text-[9px] font-medium uppercase tracking-wider text-[var(--ct-muted)]/70 select-none"
        >
          <span>{SECTION_LABEL[section]}</span>{" "}
          <span className="ml-1 tabular-nums">{count}</span>
        </div>
      )}
      {children}
    </div>
  );
}
