import type { ReactNode } from "react";
import type { Tab } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { surfaceOwnerTitle } from "../../lib/surfaceTab";
import { isTabDragDisabled, SidebarSingleTabRow, type SidebarSingleTabRowProps } from "./SidebarPlanPeerPair";
import { SurfaceOwnerBlock } from "./SurfaceAttachments";
import { attachmentInset, ownerRowInset, type OwnerRowContext } from "./surfaceAttachmentModel";
import { SortableContext, SortableRow, TAB_PREFIX, verticalListSortingStrategy } from "./sortable";
import { terminalOrigin } from "./teamRowLabels";

export interface SidebarOwnerTabRowProps extends Omit<SidebarSingleTabRowProps, "activity" | "isClosing" | "isAppearing"> {
  tabs: Record<string, Tab>;
  planIndex: PlanIndexEntry[];
  attachments: readonly Tab[];
  closingTabIds: ReadonlySet<string>;
  appearingTabIds: ReadonlySet<string>;
  compact: boolean;
  context: OwnerRowContext;
  renderOwner?: Parameters<typeof SurfaceOwnerBlock>[0]["renderOwner"];
  /** False when an enclosing sortable carries the owner together with its attachments. */
  ownerSortable?: boolean;
}

/** Drags an owner, its attached surfaces and anything nested under it as one unit. */
export function SidebarSortableBlock({ tab, group, groupKey, pinnedIds, toWorkspaceOrder, className, children }: Pick<SidebarSingleTabRowProps, "tab" | "group" | "groupKey" | "pinnedIds" | "toWorkspaceOrder"> & {
  className?: string;
  children: ReactNode;
}) {
  return (
    <SortableRow id={`${TAB_PREFIX}${tab.id}`} disabled={isTabDragDisabled(tab)} data={{ group, groupKey, pinnedIds, toWorkspaceOrder }}>
      <div className={className}>{children}</div>
    </SortableRow>
  );
}

export function SidebarOwnerTabRow({ tabs, planIndex, attachments, closingTabIds, appearingTabIds, compact, context, renderOwner, ownerSortable = true, ...row }: SidebarOwnerTabRowProps) {
  const renderRow = (tab: Tab, index: number, extra: Partial<SidebarSingleTabRowProps> = {}) => (
    <SidebarSingleTabRow {...row} tab={tab} index={index}
      ownerTitle={surfaceOwnerTitle(tab, tabs)}
      activity={resolveTabTabActivity(planIndex, tab.id, tabs)}
      isClosing={closingTabIds.has(tab.id)} isAppearing={appearingTabIds.has(tab.id)} {...extra} />
  );
  const wrapAttachments = (rows: ReactNode) => row.sortable === false ? rows : (
    <SortableContext items={attachments.map((tab) => `${TAB_PREFIX}${tab.id}`)} strategy={verticalListSortingStrategy}>{rows}</SortableContext>
  );
  return <SurfaceOwnerBlock ownerId={row.tab.id} attachments={attachments} compact={compact}
    renderOwner={renderOwner ?? ((toggle) => renderRow(row.tab, row.index, { attachmentToggle: toggle, sortable: ownerSortable && row.sortable !== false }))}
    wrapAttachments={wrapAttachments}
    renderAttachment={(tab, index) => renderRow(tab, index, {
      group: "child", groupKey: row.tab.id, pinnedIds: [], variant: "attachment",
      insetPx: attachmentInset(ownerRowInset(context, row.touchMode)), ownerBadgeTab: compact ? row.tab : null,
      startedBy: null,
    })} />;
}

export function SidebarShellRow({ showStartedBy = true, ...props }: Omit<SidebarOwnerTabRowProps, "context"> & { showStartedBy?: boolean }) {
  const creatorId = props.tab.spawned_by_tab_id ?? null;
  const creator = creatorId ? props.tabs[creatorId] : null;
  const origin = terminalOrigin(creatorId, creator?.title, Boolean(creator));
  const startedBy = origin.kind === "tab"
    ? { id: origin.id, label: origin.label, onOpen: () => props.onActivateTab(origin.id) }
    : { id: origin.kind === "closed" ? origin.id : null, label: origin.kind === "closed" ? "a closed tab" : "an unknown tab" };
  if (props.sortable === false) {
    return <div className="sidebar-team-member">
      <SidebarOwnerTabRow {...props} context="team" startedBy={showStartedBy ? startedBy : undefined} />
    </div>;
  }
  return <SidebarSortableBlock {...props} className="sidebar-team-member">
    <SidebarOwnerTabRow {...props} context="team" ownerSortable={false} startedBy={showStartedBy ? startedBy : undefined} />
  </SidebarSortableBlock>;
}
