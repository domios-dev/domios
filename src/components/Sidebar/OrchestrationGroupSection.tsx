import { useState, type ReactNode } from "react";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { type OrchestrationSection } from "./orchestrationSections";

function sectionTestId(section: OrchestrationSection): string {
  return `group-section-${section}s`;
}

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
  const [expanded, setExpanded] = useState(true);
  if (count <= 0) return null;
  return (
    <div data-group-section-wrap={section}>
      {showHeader && (
        <SidebarSectionHeader kind={section === "manager" ? "managers" : "workers"} count={count} expanded={expanded} onClick={() => setExpanded(value => !value)} data-testid={sectionTestId(section)} data-group-section={section} />
      )}
      {(!showHeader || expanded) && children}
    </div>
  );
}
