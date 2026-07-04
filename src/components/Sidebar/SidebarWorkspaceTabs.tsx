import type { ReactNode } from "react";
import { Plus } from "lucide-react";
import type { Pane, PaneStatus, Tab, Workspace } from "../../lib/types";
import type { RemoteWorkersByOrchestrator } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Plan } from "../Canvas/types";
import type { ActiveWorkSnapshot } from "../Canvas/planView/activeWork";
import { resolvePaneTabActivity } from "../Canvas/paneTabActivity";
import { WorkspacePlanTreeSlot } from "./WorkspacePlanEntry";
import { RemoteWorkerRow } from "./RemoteWorkerRow";
import {
  SortableContext,
  verticalListSortingStrategy,
  TAB_PREFIX,
} from "./sortable";
import { childTabsForParent, collectPaneIds, topLevelTabsInWorkspace } from "./workspaceTabs";
import { buildHierarchySubtree, HierarchyTree } from "./HierarchyTree";
import { isWorkspacePlanEntryActive } from "./sidebarSelection";
import {
  persistTabRename,
  resolvePlannerPeerTabContext,
  SidebarPlanPeerPairTabRows,
  SidebarSingleTabRow,
} from "./SidebarPlanPeerPair";
import { TabItem } from "./TabItem";
import { aggregateTabStatus } from "./sidebarTabStatus";
import { isWorkspaceTabActive } from "./sidebarSelection";

export interface SidebarWorkspaceTabsProps {
  workspace: Workspace;
  activeTabId: string | null;
  sortedTabs: Tab[];
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
  planIndex: PlanIndexEntry[];
  plans: Plan[];
  activeWork: ActiveWorkSnapshot | null;
  remoteWorkers: RemoteWorkersByOrchestrator;
  closingTabIds: Set<string>;
  appearingTabIds: Set<string>;
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  touchMode: boolean;
  uiFontScale?: number;
  compact: boolean;
  sidebarActionText: string;
  orchestrators: Pane[];
  onBeforeOpenPlan: () => void;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
  onNewSession: () => void;
}

export function SidebarWorkspaceTabs({
  workspace: ws,
  activeTabId: wsActiveTabId,
  sortedTabs,
  panes,
  paneStatuses,
  planIndex,
  plans,
  activeWork,
  remoteWorkers,
  closingTabIds,
  appearingTabIds,
  isActiveVisual,
  workspaceCanvasActive,
  touchMode,
  uiFontScale,
  compact,
  sidebarActionText,
  orchestrators,
  onBeforeOpenPlan,
  onActivateTab,
  onCloseTab,
  onRenameTab,
  onNewSession,
}: SidebarWorkspaceTabsProps) {
  const renderTab = (
    tab: Tab,
    index: number,
    depth: number,
    group: "tab" | "child",
    groupKey: string,
    items: string[],
    pinnedIds: string[],
  ): ReactNode => {
    const tabPids = collectPaneIds(tab, panes);
    const firstPane = tabPids[0] ? panes[tabPids[0]] : null;
    const tabActivity = firstPane
      ? resolvePaneTabActivity(planIndex, firstPane.id, panes)
      : null;
    const childTabs = firstPane ? childTabsForParent(sortedTabs, panes, firstPane.id) : [];
    const childIds = childTabs.map((t) => `${TAB_PREFIX}${t.id}`);

    const peerCtx =
      firstPane?.role === "Orchestrator"
        ? resolvePlannerPeerTabContext(firstPane, childTabs, plans, panes)
        : { plannerTab: null, peerExclude: undefined, showPeerPair: false };

    const sharedRow = {
      workspaceId: ws.id,
      activeTabId: wsActiveTabId,
      isActiveVisual,
      workspaceCanvasActive,
      panes,
      paneStatuses,
      orchestrators,
      touchMode,
      uiFontScale,
      onActivateTab,
      onCloseTab,
      onRenameTab: (tabId: string, title: string) => {
        onRenameTab(tabId, title);
        persistTabRename(ws.id, tabId, title);
      },
    };

    return (
      <div key={tab.id} style={depth > 0 ? { paddingLeft: depth * 12 } : undefined}>
        {peerCtx.showPeerPair && peerCtx.plannerTab && firstPane ? (
          <SidebarPlanPeerPairTabRows
            orchestratorTab={tab}
            orchestratorPane={firstPane}
            plannerTab={peerCtx.plannerTab}
            tabIndex={index}
            childTabs={childTabs}
            childIds={childIds}
            group={group}
            groupKey={groupKey}
            items={items}
            pinnedIds={pinnedIds}
            planIndex={planIndex}
            closingTabs={closingTabIds}
            appearingTabs={appearingTabIds}
            {...sharedRow}
          />
        ) : (
          <SidebarSingleTabRow
            tab={tab}
            index={index}
            pane={firstPane}
            paneIds={tabPids}
            activity={tabActivity}
            isClosing={closingTabIds.has(tab.id)}
            isAppearing={appearingTabIds.has(tab.id)}
            group={group}
            groupKey={groupKey}
            items={items}
            pinnedIds={pinnedIds}
            {...sharedRow}
          />
        )}
        {childTabs.length > 0 && firstPane && (
          firstPane.role === "Orchestrator" ? (
            <HierarchyTree
              nodes={buildHierarchySubtree(
                firstPane.id,
                panes,
                paneStatuses,
                peerCtx.peerExclude,
              )}
              depth={depth + 1}
              onJumpToPane={(paneId) => {
                const childTab = sortedTabs.find((t) => t.pane_id === paneId);
                if (childTab) onActivateTab(childTab.id);
              }}
              reportCounts={{}}
              uiFontScale={uiFontScale}
              activeWork={activeWork}
            />
          ) : (
            <SortableContext items={childIds} strategy={verticalListSortingStrategy}>
              {childTabs.map((ct, ci) =>
                renderTab(ct, ci, depth + 1, "child", firstPane.id, childIds, []),
              )}
            </SortableContext>
          )
        )}
        {firstPane &&
          remoteWorkers[firstPane.id]?.map((w) => (
            <RemoteWorkerRow
              key={`${w.peer}/${w.remote_pane}`}
              worker={w}
              depth={depth + 1}
            />
          ))}
      </div>
    );
  };

  const topLevel = topLevelTabsInWorkspace(sortedTabs, panes);
  const topLevelIds = topLevel.map((t) => `${TAB_PREFIX}${t.id}`);
  const pinnedTabIds = topLevel
    .filter((t) => panes[t.pane_id]?.protected)
    .map((t) => `${TAB_PREFIX}${t.id}`);

  const newSessionTrigger = (
    <button
      type="button"
      onClick={onNewSession}
      className="flex items-center gap-2 min-w-0 max-w-full mx-2 mt-1 mb-0.5 px-3 py-1 rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors"
      title="New session in this workspace"
    >
      <Plus size={10} strokeWidth={1.5} className="shrink-0" />
      <span className={`${sidebarActionText} min-w-0 truncate`}>New session</span>
    </button>
  );

  return (
    <>
      <WorkspacePlanTreeSlot
        workspace={ws}
        panes={panes}
        planIndex={planIndex}
        canvasActive={isWorkspacePlanEntryActive(isActiveVisual, workspaceCanvasActive)}
        touchMode={touchMode}
        compact={compact}
        uiFontScale={uiFontScale}
        onBeforeOpen={onBeforeOpenPlan}
      />
      <SortableContext items={topLevelIds} strategy={verticalListSortingStrategy}>
        {topLevel.map((tab, index) =>
          renderTab(tab, index, 0, "tab", ws.id, topLevelIds, pinnedTabIds),
        )}
      </SortableContext>
      {newSessionTrigger}
    </>
  );
}

export interface SidebarCollapsedActiveTabProps {
  workspace: Workspace;
  activeTabId: string | null;
  panes: Record<string, Pane>;
  paneStatuses: Record<string, PaneStatus>;
  planIndex: PlanIndexEntry[];
  isActiveVisual: boolean;
  workspaceCanvasActive: boolean;
  orchestrators: Pane[];
  touchMode: boolean;
  uiFontScale?: number;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
}

export function SidebarCollapsedActiveTab({
  workspace: ws,
  activeTabId,
  panes,
  paneStatuses,
  planIndex,
  isActiveVisual,
  workspaceCanvasActive,
  orchestrators,
  touchMode,
  uiFontScale,
  onActivateTab,
  onCloseTab,
  onRenameTab,
}: SidebarCollapsedActiveTabProps) {
  if (!activeTabId) return null;
  const activeTab = ws.tabs.find((t) => t.id === activeTabId);
  if (!activeTab) return null;
  const atPids = collectPaneIds(activeTab, panes);
  const atFirstPane = atPids[0] ? panes[atPids[0]] : null;
  const atActivity = atFirstPane
    ? resolvePaneTabActivity(planIndex, atFirstPane.id, panes)
    : null;

  return (
    <TabItem
      tab={activeTab}
      index={0}
      isActive={isWorkspaceTabActive(
        isActiveVisual,
        workspaceCanvasActive,
        activeTab.id,
        activeTabId ?? undefined,
      )}
      onClick={() => onActivateTab(activeTab.id)}
      onClose={() => onCloseTab(activeTab.id)}
      canClose={true}
      paneCount={atPids.length}
      workspaceId={ws.id}
      hasAgent={atPids.some((pid) => !!panes[pid]?.agent_type)}
      status={aggregateTabStatus(atPids, paneStatuses)}
      onRename={(title) => {
        onRenameTab(activeTab.id, title);
        persistTabRename(ws.id, activeTab.id, title);
      }}
      pane={atFirstPane}
      orchestrators={orchestrators}
      touchMode={touchMode}
      uiFontScale={uiFontScale}
      activity={atActivity}
    />
  );
}
