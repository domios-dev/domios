import { invoke } from "../../lib/bridge";
import type { Tab, TabStatus, Tab } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import { resolvePlanPeerPair } from "../shared/PairAffordance";
import { SortableRow, TAB_PREFIX } from "./sortable";
import { TabItem } from "./TabItem";
import { isWorkspaceTabActive } from "./sidebarSelection";

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
    plannerPeerId != null ? childTabs.find((t) => t.tab_id === plannerPeerId) ?? null : null;
  const peerExclude =
    plannerTab && plannerPeerId ? new Set<string>([plannerPeerId]) : undefined;
  return { plannerTab, peerExclude };
}

export interface SidebarTabItemRowProps {
  tab: Tab;
  index: number;
  tab: Tab | null;
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
  orchestrators: Tab[];
  touchMode: boolean;
  uiFontScale?: number;
  groupCollapsible?: boolean;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  collapsedSummary?: string | null;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

function SidebarTabItemRow({
  tab,
  index,
  tab,
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
  orchestrators,
  touchMode,
  uiFontScale,
  groupCollapsible,
  collapsed,
  onToggleCollapse,
  collapsedSummary,
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
      hasAgent={Boolean(tab?.agent_type)}
      status={tab ? tabStatuses[tab.id] ?? null : null}
      onRename={(title) => onRenameTab(tab.id, title)}
      tab={tab}
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
    />
  );
}

export interface SidebarSingleTabRowProps extends SidebarTabItemRowProps {
  group: "tab" | "child";
  groupKey: string;
  items: string[];
  pinnedIds: string[];
}

export function SidebarSingleTabRow({
  group,
  groupKey,
  items,
  pinnedIds,
  tab,
  ...rowProps
}: SidebarSingleTabRowProps) {
  return (
    <SortableRow
      id={`${TAB_PREFIX}${rowProps.tab.id}`}
      disabled={tab?.protected}
      data={{ group, groupKey, items, pinnedIds }}
    >
      <SidebarTabItemRow tab={tab} {...rowProps} />
    </SortableRow>
  );
}

export function persistTabRename(workspaceId: string, tabId: string, title: string): void {
  invoke("mux_rename_tab", { workspaceId, tabId, title }).catch(() => {});
}
