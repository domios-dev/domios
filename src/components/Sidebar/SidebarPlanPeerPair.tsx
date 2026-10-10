import { invoke } from "../../lib/bridge";
import type { Tab, TabStatus } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import { resolvePlanPeerPair } from "../shared/PairAffordance";
import { SortableRow, TAB_PREFIX, type SortableGroupData } from "./sortable";
import { TabItem, type AttachmentToggle, type TabStartedBy } from "./TabItem";
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
  activeWork?: ActiveWorkSnapshot | null;
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
  startedBy?: TabStartedBy | null;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

function SidebarTabItemRow({
  tab,
  index,
  activity,
  activeWork = null,
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
  startedBy,
  onActivateTab,
  onCloseTab,
  onRenameTab,
}: SidebarTabItemRowProps) {
  return (
    <TabItem
      tab={tab}
      index={index}
      isActive={isWorkspaceTabActive(
        isActiveVisual,
        workspaceCanvasActive,
        tab.id,
        activeTabId ?? undefined,
      )}
      onClick={() => onActivateTab(tab.id)}
      onClose={() => onCloseTab(tab.id)}
      canClose={true}
      workspaceId={workspaceId}
      hasAgent={Boolean(tab.agent_type)}
      status={tabStatuses[tab.id] ?? null}
      attention={tabAttentionByTab[tab.id] ?? null}
      onRename={(title) => onRenameTab(tab.id, title)}
      orchestrators={orchestrators}
      isAppearing={isAppearing}
      isClosing={isClosing}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={activity}
      activeWork={activeWork}
      reportCount={reportCount}
      groupCollapsible={groupCollapsible}
      collapsed={collapsed}
      onToggleCollapse={onToggleCollapse}
      collapsedSummary={collapsedSummary}
      ownerTitle={ownerTitle}
      variant={variant}
      insetPx={insetPx}
      ownerBadgeTab={ownerBadgeTab}
      attachmentToggle={attachmentToggle}
      startedBy={startedBy}
    />
  );
}

export interface SidebarSingleTabRowProps extends SidebarTabItemRowProps {
  group: "tab" | "team" | "child";
  groupKey: string;
  pinnedIds: string[];
  toWorkspaceOrder?: SortableGroupData["toWorkspaceOrder"];
}

export function isTabDragDisabled(tab: Pick<Tab, "protected">): boolean {
  return tab.protected === true;
}

export function SidebarSingleTabRow({
  group,
  groupKey,
  pinnedIds,
  toWorkspaceOrder,
  ...rowProps
}: SidebarSingleTabRowProps) {
  return (
    <SortableRow
      id={`${TAB_PREFIX}${rowProps.tab.id}`}
      disabled={isTabDragDisabled(rowProps.tab)}
      data={{ group, groupKey, pinnedIds, toWorkspaceOrder }}
    >
      <SidebarTabItemRow {...rowProps} />
    </SortableRow>
  );
}

export function persistTabRename(workspaceId: string, tabId: string, title: string): void {
  invoke("mux_rename_tab", { workspaceId, tabId, title }).catch(() => {});
}
