import type { ButtonHTMLAttributes } from "react";
import { ActivityDots } from "../shared/ActivityDots";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { sidebarSectionModel, type SidebarSectionKind } from "./sidebarSectionModel";

interface SidebarSectionHeaderProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-expanded"> {
  kind: SidebarSectionKind;
  count: number;
  working?: number;
  expanded: boolean;
  touchMode?: boolean;
}

export function SidebarSectionHeader({ kind, count, working = 0, expanded, touchMode, ...props }: SidebarSectionHeaderProps) {
  const model = sidebarSectionModel(kind, count, working);
  return <SidebarDisclosure {...props} expanded={expanded} touchMode={touchMode} aria-label={model.accessibleName} className="sidebar-team-workers-toggle">
    <span>{model.label}</span>
    {model.activity && <span className="sidebar-team-workers-state"><span aria-hidden>·</span><span>{model.activity}</span><ActivityDots size="small" tone="muted" active /></span>}
  </SidebarDisclosure>;
}
