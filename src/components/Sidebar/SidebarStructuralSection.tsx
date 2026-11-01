import type { ReactNode } from "react";
import { SidebarDisclosure } from "./SidebarDisclosure";

/** A top-level sidebar section below General: one sticky collapsible header and its body. */
export function SidebarStructuralSection({ label, ariaLabel, expanded, onToggle, testId, action, children, firstRunMuted = false }: {
  label: string;
  ariaLabel?: string;
  expanded: boolean;
  onToggle: () => void;
  testId: string;
  action?: ReactNode;
  children?: ReactNode;
  firstRunMuted?: boolean;
}) {
  return <section className={`sidebar-structural-section ${firstRunMuted ? "opacity-55 text-[var(--ct-muted-fg)]" : ""}`}
    data-first-run-muted={firstRunMuted || undefined} aria-label={ariaLabel}>
    <div className="sidebar-structural-header">
      <SidebarDisclosure data-testid={testId} className="sidebar-structural-toggle" expanded={expanded} onClick={onToggle}>
        <span className="sidebar-structural-label">{label}</span>
      </SidebarDisclosure>
      <div className="sidebar-structural-action">{action}</div>
    </div>
    {children}
  </section>;
}
