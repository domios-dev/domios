import { memo, type ReactNode } from "react";
import type { Tab } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { resolveTabTabActivity } from "../Canvas/tabTabActivity";
import { surfaceOwnerTitle } from "../../lib/surfaceTab";
import { isTabDragDisabled, sameIds, sameTabItemRow, SidebarSingleTabRow, type SidebarSingleTabRowProps, type SidebarTabItemRowProps } from "./SidebarPlanPeerPair";
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
      startedBy: null, checklistToggle: null,
    })} />;
}

type SidebarShellRowProps = Omit<SidebarOwnerTabRowProps, "context"> & {
  showStartedBy?: boolean;
  /** Row detail that stays inside the member block, below its row. */
  detail?: ReactNode;
};

function creatorLabel({ tab, tabs }: SidebarShellRowProps): [string | null, string | null, boolean] {
  const creator = tab.spawned_by_tab_id ? tabs[tab.spawned_by_tab_id] : undefined;
  return [tab.spawned_by_tab_id ?? null, creator?.title ?? null, Boolean(creator)];
}

function rowAt(props: SidebarShellRowProps, tab: Tab, index: number): SidebarTabItemRowProps {
  return {
    ...props,
    tab,
    index,
    ownerTitle: surfaceOwnerTitle(tab, props.tabs),
    activity: resolveTabTabActivity(props.planIndex, tab.id, props.tabs),
    isClosing: props.closingTabIds.has(tab.id),
    isAppearing: props.appearingTabIds.has(tab.id),
  };
}

/** Same member block: placement, creator, and every row of the owner and its attachments. */
function sameShellRow(prev: SidebarShellRowProps, next: SidebarShellRowProps): boolean {
  if (prev.showStartedBy !== next.showStartedBy || prev.detail !== next.detail || prev.compact !== next.compact
    || prev.renderOwner !== next.renderOwner || prev.ownerSortable !== next.ownerSortable || prev.sortable !== next.sortable
    || prev.group !== next.group || prev.groupKey !== next.groupKey || prev.toWorkspaceOrder !== next.toWorkspaceOrder
    || !sameIds(prev.pinnedIds, next.pinnedIds) || prev.attachments.length !== next.attachments.length) return false;
  if (creatorLabel(prev).some((value, index) => value !== creatorLabel(next)[index])) return false;
  const prevTabs = [prev.tab, ...prev.attachments];
  const nextTabs = [next.tab, ...next.attachments];
  return prevTabs.every((tab, index) => tab.id === nextTabs[index].id
    && sameTabItemRow(rowAt(prev, tab, index === 0 ? prev.index : index - 1), rowAt(next, nextTabs[index], index === 0 ? next.index : index - 1)));
}

/** A team member block that renders only when one of its own rows changes. */
export const SidebarShellRow = memo(function SidebarShellRow({ showStartedBy = true, detail = null, ...props }: SidebarShellRowProps) {
  const creatorId = props.tab.spawned_by_tab_id ?? null;
  const creator = creatorId ? props.tabs[creatorId] : null;
  const origin = terminalOrigin(creatorId, creator?.title, Boolean(creator));
  const startedBy = origin.kind === "tab"
    ? { id: origin.id, label: origin.label, onOpen: () => props.onActivateTab(origin.id) }
    : { id: origin.kind === "closed" ? origin.id : null, label: origin.kind === "closed" ? "a closed tab" : "an unknown tab" };
  if (props.sortable === false) {
    return <div className="sidebar-team-member">
      <SidebarOwnerTabRow {...props} context="team" startedBy={showStartedBy ? startedBy : undefined} />
      {detail}
    </div>;
  }
  return <SidebarSortableBlock {...props} className="sidebar-team-member">
    <SidebarOwnerTabRow {...props} context="team" ownerSortable={false} startedBy={showStartedBy ? startedBy : undefined} />
    {detail}
  </SidebarSortableBlock>;
}, sameShellRow);
