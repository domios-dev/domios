import { invoke } from "../../lib/bridge";
import type { Pane, PaneStatus, Tab } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import { resolvePaneTabActivity } from "../Canvas/paneTabActivity";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { PairAffordance, resolvePlanPeerPair } from "../shared/PairAffordance";
import { SortableRow, TAB_PREFIX } from "./sortable";
import { TabItem } from "./TabItem";
import { aggregateTabStatus } from "./sidebarTabStatus";
import { isWorkspaceTabActive } from "./sidebarSelection";
import { collectPaneIds } from "./workspaceTabs";

export interface PlannerPeerTabContext {
  plannerTab: Tab | null;
  peerExclude: Set<string> | undefined;
  showPeerPair: boolean;
}

export function resolvePlannerPeerTabContext(
  orchestratorPane: Pane,
  childTabs: Tab[],
  plans: Plan[],
  panes: Record<string, Pane>,
): PlannerPeerTabContext {
  const peerPair = resolvePlanPeerPair(orchestratorPane.id, plans, panes);
  const plannerPeerId = peerPair?.plannerPaneId ?? null;
  const plannerTab =
    plannerPeerId != null ? childTabs.find((t) => t.pane_id === plannerPeerId) ?? null : null;
  const showPeerPair = Boolean(plannerTab);
  const peerExclude =
    showPeerPair && plannerPeerId ? new Set<string>([plannerPeerId]) : undefined;
  return { plannerTab, peerExclude, showPeerPair };
}

export interface SidebarTabItemRowProps {
  tab: Tab;
  index: number;
  pane: Pane | null;
  paneIds: string[];
  activity: PaneTabActivity | null;
  isClosing: boolean;
  isAppearing: boolean;
  workspaceId: string;
  activeTabId: string | null;
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
  orchestrators: Pane[];
  touchMode: boolean;
  uiFontScale?: number;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

function SidebarTabItemRow({
  tab,
  index,
  pane,
  paneIds,
  activity,
  isClosing,
  isAppearing,
  workspaceId,
  activeTabId,
  isActiveVisual,
  workspaceCanvasActive,
  panes,
  paneStatuses,
  orchestrators,
  touchMode,
  uiFontScale,
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
      paneCount={paneIds.length}
      workspaceId={workspaceId}
      hasAgent={paneIds.some((pid) => !!panes[pid]?.agent_type)}
      status={aggregateTabStatus(paneIds, paneStatuses)}
      onRename={(title) => onRenameTab(tab.id, title)}
      pane={pane}
      orchestrators={orchestrators}
      isAppearing={isAppearing}
      isClosing={isClosing}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={activity}
    />
  );
}

export interface SidebarPlanPeerPairTabRowsProps {
  orchestratorTab: Tab;
  orchestratorPane: Pane;
  plannerTab: Tab;
  tabIndex: number;
  childTabs: Tab[];
  childIds: string[];
  group: "tab" | "child";
  groupKey: string;
  items: string[];
  pinnedIds: string[];
  planIndex: PlanIndexEntry[];
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
  orchestrators: Pane[];
  closingTabs: Set<string>;
  appearingTabs: Set<string>;
  workspaceId: string;
  activeTabId: string | null;
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  touchMode: boolean;
  uiFontScale?: number;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

export function SidebarPlanPeerPairTabRows({
  orchestratorTab,
  orchestratorPane,
  plannerTab,
  tabIndex,
  childTabs,
  childIds,
  group,
  groupKey,
  items,
  pinnedIds,
  planIndex,
  panes,
  paneStatuses,
  orchestrators,
  closingTabs,
  appearingTabs,
  workspaceId,
  activeTabId,
  isActiveVisual,
  workspaceCanvasActive,
  touchMode,
  uiFontScale,
  onActivateTab,
  onCloseTab,
  onRenameTab,
}: SidebarPlanPeerPairTabRowsProps) {
  const orchPids = collectPaneIds(orchestratorTab, panes);
  const sharedRow = {
    workspaceId,
    activeTabId,
    isActiveVisual,
    workspaceCanvasActive,
    panes,
    paneStatuses,
    orchestrators,
    touchMode,
    uiFontScale,
    onActivateTab,
    onCloseTab,
    onRenameTab,
  };

  return (
    <PairAffordance
      layout="sidebar"
      data-testid="sidebar-peer-pair"
      orchestrator={
        <SortableRow
          id={`${TAB_PREFIX}${orchestratorTab.id}`}
          disabled={orchestratorPane.protected}
          data={{ group, groupKey, items, pinnedIds }}
        >
          <SidebarTabItemRow
            tab={orchestratorTab}
            index={tabIndex}
            pane={orchestratorPane}
            paneIds={orchPids}
            activity={resolvePaneTabActivity(planIndex, orchestratorPane.id, panes)}
            isClosing={closingTabs.has(orchestratorTab.id)}
            isAppearing={appearingTabs.has(orchestratorTab.id)}
            {...sharedRow}
          />
        </SortableRow>
      }
      planner={
        <SortableRow
          id={`${TAB_PREFIX}${plannerTab.id}`}
          disabled={panes[plannerTab.pane_id]?.protected}
          data={{ group: "child", groupKey: orchestratorPane.id, items: childIds, pinnedIds: [] }}
        >
          <SidebarTabItemRow
            tab={plannerTab}
            index={childTabs.indexOf(plannerTab)}
            pane={panes[plannerTab.pane_id] ?? null}
            paneIds={collectPaneIds(plannerTab, panes)}
            activity={resolvePaneTabActivity(planIndex, plannerTab.pane_id, panes)}
            isClosing={closingTabs.has(plannerTab.id)}
            isAppearing={appearingTabs.has(plannerTab.id)}
            {...sharedRow}
          />
        </SortableRow>
      }
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
  pane,
  ...rowProps
}: SidebarSingleTabRowProps) {
  return (
    <SortableRow
      id={`${TAB_PREFIX}${rowProps.tab.id}`}
      disabled={pane?.protected}
      data={{ group, groupKey, items, pinnedIds }}
    >
      <SidebarTabItemRow pane={pane} {...rowProps} />
    </SortableRow>
  );
}

export function persistTabRename(workspaceId: string, tabId: string, title: string): void {
  invoke("mux_rename_tab", { workspaceId, tabId, title }).catch(() => {});
}
