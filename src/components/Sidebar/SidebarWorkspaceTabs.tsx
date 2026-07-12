import { useCallback, useState, type ReactNode } from "react";
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
  SidebarSingleTabRow,
} from "./SidebarPlanPeerPair";
import { OrchestrationGroupSection } from "./OrchestrationGroupSection";
import { collectOrchestrationGroupMembers } from "./orchestrationGroupClose";
import {
  countBySection,
  shouldShowSectionHeader,
  summarizeGroupCounts,
} from "./orchestrationSections";
import { TabItem } from "./TabItem";
import { aggregateTabStatus } from "./sidebarTabStatus";
import { isWorkspaceTabActive } from "./sidebarSelection";

function orderHierarchyNodes(
  nodes: ReturnType<typeof buildHierarchySubtree>,
  tabIndexByPaneId: ReadonlyMap<string, number>,
): ReturnType<typeof buildHierarchySubtree> {
  return [...nodes]
    .sort(
      (a, b) =>
        (tabIndexByPaneId.get(a.pane.id) ?? Number.MAX_SAFE_INTEGER) -
        (tabIndexByPaneId.get(b.pane.id) ?? Number.MAX_SAFE_INTEGER),
    )
    .map((node) => ({
      ...node,
      children: orderHierarchyNodes(node.children, tabIndexByPaneId),
    }));
}

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
  const [collapsedGroups, setCollapsedGroups] = useState<ReadonlySet<string>>(() => new Set());
  const toggleGroupCollapsed = useCallback((orchestratorPaneId: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(orchestratorPaneId)) next.delete(orchestratorPaneId);
      else next.add(orchestratorPaneId);
      return next;
    });
  }, []);

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
        : { plannerTab: null, peerExclude: undefined };

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

    const isOrchestrator = firstPane?.role === "Orchestrator";
    const plannerTab = peerCtx.plannerTab;
    const managerTabs =
      isOrchestrator && firstPane
        ? childTabs
            .filter((t) => panes[t.pane_id]?.role === "Manager")
            .sort((a, b) => {
              if (plannerTab?.id === a.id) return -1;
              if (plannerTab?.id === b.id) return 1;
              return childTabs.indexOf(a) - childTabs.indexOf(b);
            })
        : [];
    const managerPaneIds = new Set(managerTabs.map((t) => t.pane_id));
    const managerIds = managerTabs.map((t) => `${TAB_PREFIX}${t.id}`);
    const workerExclude = new Set<string>(peerCtx.peerExclude ?? []);
    for (const paneId of managerPaneIds) {
      workerExclude.add(paneId);
    }
    const workerNodes =
      isOrchestrator && firstPane
        ? buildHierarchySubtree(firstPane.id, panes, paneStatuses, workerExclude)
        : [];
    const childTabIndexByPaneId = new Map(childTabs.map((t, i) => [t.pane_id, i]));
    const orderedWorkerNodes = orderHierarchyNodes(workerNodes, childTabIndexByPaneId);
    const workerIds = orderedWorkerNodes
      .map((node) => sortedTabs.find((t) => t.pane_id === node.pane.id))
      .filter((t): t is Tab => Boolean(t))
      .map((t) => `${TAB_PREFIX}${t.id}`);
    const sectionCounts =
      isOrchestrator && firstPane
        ? countBySection(collectOrchestrationGroupMembers(firstPane.id, panes))
        : { managers: 0, workers: 0 };

    const groupChildCount = sectionCounts.managers + sectionCounts.workers;
    const groupCollapsed = isOrchestrator && firstPane ? collapsedGroups.has(firstPane.id) : false;
    const groupSummary =
      isOrchestrator && firstPane && groupChildCount > 0
        ? summarizeGroupCounts(sectionCounts)
        : null;

    return (
      <div
        key={tab.id}
        data-orchestration-group={isOrchestrator && firstPane ? firstPane.id : undefined}
        className={isOrchestrator ? "sidebar-orchestration-group" : undefined}
        style={depth > 0 ? { paddingLeft: depth * 12 } : undefined}
      >
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
          groupCollapsible={isOrchestrator && groupChildCount > 0}
          collapsed={groupCollapsed}
          onToggleCollapse={
            isOrchestrator && firstPane ? () => toggleGroupCollapsed(firstPane.id) : undefined
          }
          collapsedSummary={groupSummary}
          activeWork={activeWork}
          {...sharedRow}
        />
        {isOrchestrator && firstPane ? (
          !groupCollapsed && (
            <div
              data-testid="group-container"
              data-group-container={firstPane.id}
              className="sidebar-group-container"
            >
              <OrchestrationGroupSection
                section="manager"
                count={sectionCounts.managers}
                showHeader={shouldShowSectionHeader("manager", sectionCounts)}
              >
                {managerTabs.length > 0 && (
                  <SortableContext items={managerIds} strategy={verticalListSortingStrategy}>
                    {managerTabs.map((managerTab) => (
                      <SidebarSingleTabRow
                        key={managerTab.id}
                        tab={managerTab}
                        index={childTabs.indexOf(managerTab)}
                        pane={panes[managerTab.pane_id] ?? null}
                        paneIds={collectPaneIds(managerTab, panes)}
                        activity={resolvePaneTabActivity(planIndex, managerTab.pane_id, panes)}
                        activeWork={activeWork}
                        isClosing={closingTabIds.has(managerTab.id)}
                        isAppearing={appearingTabIds.has(managerTab.id)}
                        group="child"
                        groupKey={firstPane.id}
                        items={childIds}
                        pinnedIds={[]}
                        {...sharedRow}
                      />
                    ))}
                  </SortableContext>
                )}
              </OrchestrationGroupSection>
              <OrchestrationGroupSection
                section="worker"
                count={sectionCounts.workers}
                showHeader={shouldShowSectionHeader("worker", sectionCounts)}
              >
                {orderedWorkerNodes.length > 0 && (
                  <SortableContext items={workerIds} strategy={verticalListSortingStrategy}>
                    <HierarchyTree
                      nodes={orderedWorkerNodes}
                      depth={0}
                      renderRow={(node) => {
                        const childTab = sortedTabs.find((t) => t.pane_id === node.pane.id);
                        if (!childTab) return null;
                        return (
                          <SidebarSingleTabRow
                            key={childTab.id}
                            tab={childTab}
                            index={childTabs.indexOf(childTab)}
                            pane={node.pane}
                            paneIds={collectPaneIds(childTab, panes)}
                            activity={resolvePaneTabActivity(planIndex, node.pane.id, panes)}
                            activeWork={activeWork}
                            reportCount={0}
                            isClosing={closingTabIds.has(childTab.id)}
                            isAppearing={appearingTabIds.has(childTab.id)}
                            group="child"
                            groupKey={firstPane.id}
                            items={childIds}
                            pinnedIds={[]}
                            {...sharedRow}
                          />
                        );
                      }}
                    />
                  </SortableContext>
                )}
              </OrchestrationGroupSection>
            </div>
          )
        ) : (
          childTabs.length > 0 &&
          firstPane && (
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
      data-testid="sidebar-new-session"
      onClick={onNewSession}
      className={`flex items-center gap-2 min-w-0 max-w-full mx-2 mt-1 mb-0.5 px-3 ${touchMode ? "min-h-[44px] py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] disabled:opacity-40 disabled:hover:bg-transparent transition-colors`}
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
