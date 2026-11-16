import type { ButtonHTMLAttributes } from "react";
import { ActivityDots } from "../shared/ActivityDots";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { sidebarSectionModel, type SidebarSectionKind } from "./sidebarSectionModel";
import type { GroupAttention } from "./tabAttention";

interface SidebarSectionHeaderProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-expanded"> {
  kind: SidebarSectionKind;
  count: number;
  working?: number;
  expanded: boolean;
  touchMode?: boolean;
  /** Member marks a collapsed list hides; shown on the header only while collapsed. */
  attention?: GroupAttention | null;
}

export function SidebarSectionHeader({ kind, count, working = 0, expanded, touchMode, attention, ...props }: SidebarSectionHeaderProps) {
  const hidden = expanded ? null : attention ?? null;
  const model = sidebarSectionModel(kind, count, working, hidden?.count ?? 0);
  return <SidebarDisclosure {...props} expanded={expanded} touchMode={touchMode} aria-label={model.accessibleName} className="sidebar-team-workers-toggle">
    <span>{model.label}</span>
    {hidden && <span className="tab-attention-mark sidebar-section-attention-mark" data-kind={hidden.level} aria-hidden />}
    {model.activity && <span className="sidebar-team-workers-state"><span aria-hidden>·</span><span>{model.activity}</span><ActivityDots size="small" tone="muted" active /></span>}
  </SidebarDisclosure>;
}
