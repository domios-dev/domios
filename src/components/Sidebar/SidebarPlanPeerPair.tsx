import { invoke } from "../../lib/bridge";
import type { Pane, PaneStatus, Tab } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import { resolvePlanPeerPair } from "../shared/PairAffordance";
import { SortableRow, TAB_PREFIX } from "./sortable";
import { TabItem } from "./TabItem";
import { aggregateTabStatus } from "./sidebarTabStatus";
import { isWorkspaceTabActive } from "./sidebarSelection";

export interface PlannerPeerTabContext {
  plannerTab: Tab | null;
  /** Planner pane id to keep out of the Workers subtree — it renders under
   *  Managers instead. Undefined when no planner is bound. */
  peerExclude: Set<string> | undefined;
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
  const peerExclude =
    plannerTab && plannerPeerId ? new Set<string>([plannerPeerId]) : undefined;
  return { plannerTab, peerExclude };
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
