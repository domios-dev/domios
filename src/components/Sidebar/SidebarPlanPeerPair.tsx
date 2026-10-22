import { memo } from "react";
import type { Tab, TabStatus } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import { resolvePlanPeerPair } from "../shared/PairAffordance";
import { SortableRow, TAB_PREFIX, type SortableGroupData } from "./sortable";
import { TabItem, tabItemPropsEqual, type AttachmentToggle, type TabItemState, type TabStartedBy } from "./TabItem";
import type { ChecklistToggle } from "./AgentChecklist";
import { isWorkspaceTabActive } from "./sidebarSelection";
import type { TabAttention } from "./tabAttention";

export interface PlannerPeerTabContext {
  plannerTab: Tab | null;
  /** Planner tab id to keep out of the Workers subtree — it renders under
   *  Managers instead. Undefined when no planner is bound. */
  peerExclude: Set<string> | undefined;
}

export function resolvePlannerPeerTabContext(
  orchestratorTab: Tab,
  childTabs: Tab[],
  plans: Plan[],
  tabs: Record<string, Tab>,
): PlannerPeerTabContext {
  const peerPair = resolvePlanPeerPair(orchestratorTab.id, plans, tabs);
  const plannerPeerId = peerPair?.plannerTabId ?? null;
  const plannerTab =
    plannerPeerId != null ? childTabs.find((tab) => tab.id === plannerPeerId) ?? null : null;
  const peerExclude =
    plannerTab && plannerPeerId ? new Set<string>([plannerPeerId]) : undefined;
  return { plannerTab, peerExclude };
}

export interface SidebarTabItemRowProps {
  tab: Tab;
  index: number;
  activity: TabTabActivity | null;
  reportCount?: number;
  isClosing: boolean;
  isAppearing: boolean;
  workspaceId: string;
  activeTabId: string | null;
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  tabStatuses: Record<string, TabStatus>;
  tabAttentionByTab?: Readonly<Record<string, TabAttention>>;
  orchestrators: Tab[];
  touchMode: boolean;
  uiFontScale?: number;
  groupCollapsible?: boolean;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  collapsedSummary?: string | null;
  ownerTitle?: string | null;
  variant?: "attachment";
  insetPx?: number;
  ownerBadgeTab?: Tab | null;
  attachmentToggle?: AttachmentToggle | null;
  checklistToggle?: ChecklistToggle | null;
  startedBy?: TabStartedBy | null;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

/** The TabItem state one row resolves from the shared workspace records. */
function tabItemState({
  tab,
  index,
  activity,
  reportCount = 0,
  isClosing,
  isAppearing,
  workspaceId,
  activeTabId,
  isActiveVisual,
  workspaceCanvasActive,
  tabStatuses,
  tabAttentionByTab = {},
  orchestrators,
  touchMode,
  uiFontScale,
  groupCollapsible,
  collapsed,
  onToggleCollapse,
  collapsedSummary,
  ownerTitle,
  variant,
  insetPx,
  ownerBadgeTab,
  attachmentToggle,
  checklistToggle,
  startedBy,
}: SidebarTabItemRowProps): TabItemState {
  return {
    tab,
    index,
    isActive: isWorkspaceTabActive(isActiveVisual, workspaceCanvasActive, tab.id, activeTabId ?? undefined),
    canClose: true,
    workspaceId,
    hasAgent: Boolean(tab.agent_type),
    status: tabStatuses[tab.id] ?? null,
    attention: tabAttentionByTab[tab.id] ?? null,
    orchestrators,
    isAppearing,
    isClosing,
    touchMode,
    uiFontScale,
    activity,
    reportCount,
    groupCollapsible,
    collapsed,
    onToggleCollapse,
    collapsedSummary,
    ownerTitle,
    variant,
    insetPx,
    ownerBadgeTab,
    attachmentToggle,
    checklistToggle,
    startedBy,
  };
}

function SidebarTabItemRow(props: SidebarTabItemRowProps) {
  const { tab, onActivateTab, onCloseTab, onRenameTab } = props;
  return (
    <TabItem
      {...tabItemState(props)}
      onClick={() => onActivateTab(tab.id)}
      onClose={() => onCloseTab(tab.id)}
      onRename={(title) => onRenameTab(tab.id, title)}
    />
  );
}

/** Same rendered row: the row's own TabItem state, handlers and sortable placement. */
export function sameTabItemRow(prev: SidebarTabItemRowProps, next: SidebarTabItemRowProps): boolean {
  return prev.onActivateTab === next.onActivateTab
    && prev.onCloseTab === next.onCloseTab
    && prev.onRenameTab === next.onRenameTab
    && prev.workspaceId === next.workspaceId
    && tabItemPropsEqual(tabItemState(prev), tabItemState(next));
}

export function sameIds(a: readonly string[], b: readonly string[]): boolean {
  return a === b || (a.length === b.length && a.every((id, index) => id === b[index]));
}

function sameSingleTabRow(prev: SidebarSingleTabRowProps, next: SidebarSingleTabRowProps): boolean {
  return prev.sortable === next.sortable
    && prev.group === next.group
    && prev.groupKey === next.groupKey
    && prev.toWorkspaceOrder === next.toWorkspaceOrder
    && sameIds(prev.pinnedIds, next.pinnedIds)
    && sameTabItemRow(prev, next);
}

export interface SidebarSingleTabRowProps extends SidebarTabItemRowProps {
  sortable?: boolean;
  group: "tab" | "team" | "child";
  groupKey: string;
  pinnedIds: string[];
  toWorkspaceOrder?: SortableGroupData["toWorkspaceOrder"];
}

export function isTabDragDisabled(tab: Pick<Tab, "protected">): boolean {
  return tab.protected === true;
}

/** Renders only when its own row changes; callers pass stable handlers and `toWorkspaceOrder`. */
export const SidebarSingleTabRow = memo(function SidebarSingleTabRow({
  sortable = true,
  group,
  groupKey,
  pinnedIds,
  toWorkspaceOrder,
  ...rowProps
}: SidebarSingleTabRowProps) {
  if (!sortable) return <SidebarTabItemRow {...rowProps} />;
  return (
    <SortableRow
      id={`${TAB_PREFIX}${rowProps.tab.id}`}
      disabled={isTabDragDisabled(rowProps.tab)}
      data={{ group, groupKey, pinnedIds, toWorkspaceOrder }}
    >
      <SidebarTabItemRow {...rowProps} />
    </SortableRow>
  );
}, sameSingleTabRow);
