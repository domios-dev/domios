import { useMemo } from "react";
import { SidebarDisclosure } from "../../../../src/components/Sidebar/SidebarDisclosure";
import { SidebarDrawer } from "../../../../src/components/Sidebar/SidebarDrawer";
import { MemoryGraphIcon, MeshIcon, NeighborsIcon, SidebarNavButton } from "../../../../src/components/Sidebar/SidebarNavButton";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "../../../../src/components/Sidebar/SidebarWorkspaceDetail";
import { WorkspaceCardGrid } from "../../../../src/components/Sidebar/WorkspaceCardGrid";
import { SidebarDnd } from "../../../../src/components/Sidebar/sortable";
import { LiquidFlip } from "../../../../src/components/shared/LiquidToggle";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { DEMO_PROVIDERS, ProductProviders } from "../../gallery/fixtures";
import type { View } from "./nav";
import { MEDIA_ACTIVE_PLAN, mediaPlanIndexOf, planIndexOf, type Machine } from "./story";

export const SIDEBAR_WIDTH = 280;

const noop = () => undefined;
const NONE = new Set<string>();

export interface SidebarProps {
  machine: Machine;
  workspaceId: string;
  activeTabId: string;
  tabs: Tab[];
  statuses: Record<string, TabStatus>;
  plan: Plan;
  view: View;
  phone: boolean;
  shown: boolean;
  onWorkspace: (id: string) => void;
  onTab: (id: string) => void;
  onCreateWorkspace: () => void;
  onCreateTeam: () => void;
  onView: (view: View) => void;
  notInDemo: (what: string) => void;
}

// What each sidebar action the demo does not run is called in its "Not in the demo" note.
const ACTION_NAMES: Record<string, string> = {
  closeTab: "Closing a tab", renameTab: "Renaming a tab", renameWorkspace: "Renaming a workspace", closeWorkspace: "Closing a workspace",
  newTerminal: "A new terminal", newSession: "A new session", newAgent: "A new agent", openRemoteTab: "Opening a remote tab",
  openMap: "The workspace map", renamePlan: "Renaming a plan", renameTeam: "Renaming a team", addTeamMember: "Adding a team member", closeTeam: "Closing a team",
};

/** Sidebar.tsx's expanded tree for the selected machine: its workspaces, the selected workspace's teams and tabs, then the footer views. */
export function WorkspaceSidebar({ machine, workspaceId, activeTabId, tabs, statuses, plan, view, phone, shown, onWorkspace, onTab, onCreateWorkspace, onCreateTeam, onView, notInDemo }: SidebarProps) {
  const workspace = machine.workspaces.find((ws) => ws.id === workspaceId) ?? machine.workspaces[0];
  const nav = { compact: false, touchMode: phone, labelClassName: "text-[12px]" };
  const actions = useMemo(() => new Proxy({
    activateWorkspace: onWorkspace,
    activateTab: (_ws: string, id: string) => onTab(id),
    openAutomation: () => onView("jobs"),
    newSession: onCreateTeam,
  }, {
    get: (target, key) => (target as Record<string | symbol, unknown>)[key] ?? (() => notInDemo(ACTION_NAMES[String(key)] ?? "This")),
  }) as unknown as SidebarWorkspaceDetailActions, [onWorkspace, onTab, onCreateTeam, onView, notInDemo]);
  const statusOf = (id: string): TabStatus | null => {
    const members = tabs.filter((tab) => tab.workspace_id === id);
    return members.length === 0 ? null : members.some((tab) => statuses[tab.id] === "Working") ? "Working" : "Idle";
  };
  const workspaceStatuses = Object.fromEntries(machine.workspaces.map((ws) => [ws.id, statusOf(ws.id)]));
  const owned = plan.team_id && machine.teams.some((team) => team.id === plan.team_id);
  const media = machine.teams.some((team) => team.id === MEDIA_ACTIVE_PLAN.team_id);
  const data: SidebarWorkspaceDetailData = {
    tabs: Object.fromEntries(tabs.map((tab) => [tab.id, tab])),
    tabStatuses: statuses,
    attention: {},
    teams: machine.teams,
    planIndex: [...(owned ? [planIndexOf(plan)] : []), ...(media ? [mediaPlanIndexOf(MEDIA_ACTIVE_PLAN)] : [])],
    plans: [...(owned ? [plan] : []), ...(media ? [MEDIA_ACTIVE_PLAN] : [])],
    providers: DEMO_PROVIDERS,
    providersLoading: false,
    closingTabIds: NONE,
    closingTeamIds: NONE,
    activeTabIds: { [workspace.id]: activeTabId },
    activeWorkspaceId: workspace.id,
    isWorkspaceMode: true,
    workspaceCanvasActive: view === "plan",
    touchMode: phone,
    compact: false,
    sidebarActionText: "text-[12px]",
    orchestrators: tabs.filter((tab) => tab.role_profile === "orchestrator"),
    newSessionDisabled: false,
  };
  const sortIds = machine.workspaces.map((ws) => ws.id);
  return <ProductProviders tabs={tabs} workspaceId={workspace.id}>
    <SidebarDrawer phone={phone} shown={shown}>
      <div className="relative overflow-visible shrink-0 h-full" style={{ width: SIDEBAR_WIDTH }}>
        <aside aria-label="Sidebar" data-testid="sidebar" data-collapsed="false" className="ct-sidebar-panel shrink-0 h-full min-h-0 flex flex-col select-none overflow-x-hidden overflow-y-hidden relative" style={{ width: "100%" }}>
          <div className="ct-sidebar-content flex-1 min-w-0 overflow-y-auto overflow-x-hidden">
            <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
              <div className="sidebar-structural-section">
                <div className="sidebar-structural-header">
                  <SidebarDisclosure className="sidebar-structural-toggle" expanded onClick={noop}>
                    <span className="sidebar-structural-label">WORKSPACES</span>
                  </SidebarDisclosure>
                  <div className="sidebar-structural-action" />
                </div>
                <WorkspaceCardGrid workspaces={machine.workspaces} selectedWorkspaceId={workspace.id} statuses={workspaceStatuses} workingByWorkspace={{}} sortIds={sortIds} onSelect={onWorkspace} onCreate={onCreateWorkspace} />
                <SidebarWorkspaceDetail workspace={workspace} status={workspaceStatuses[workspace.id]} data={data} actions={actions} />
              </div>
            </SidebarDnd>
          </div>
          <div role="navigation" aria-label="Sidebar views" className="sidebar-footer flex flex-col">
            <SidebarNavButton testId="nav-memory" label="Memory" icon={<MemoryGraphIcon size={14} />} current={view === "memory"} onClick={() => onView("memory")} {...nav} />
            <div className="flex items-center gap-1 w-full">
              <SidebarNavButton testId="nav-neighbors" label="Neighborhood" icon={<NeighborsIcon size={14} />} current={view === "neighbors"}
                onClick={() => onView("neighbors")} grow {...nav} />
              <LiquidFlip aria-label="Neighbors" className="shrink-0 text-[10px]" checked onCheckedChange={() => notInDemo("Turning Neighborhood off")} />
            </div>
            <div className="flex items-center gap-1 w-full">
              <SidebarNavButton testId="nav-mesh" label="Mesh" icon={<MeshIcon size={14} />} current={view === "mesh"} grow onClick={() => onView("mesh")} {...nav} />
              <LiquidFlip aria-label="Mesh presence" className="shrink-0 text-[10px]" checked onCheckedChange={() => notInDemo("Turning mesh presence off")} />
            </div>
          </div>
        </aside>
      </div>
    </SidebarDrawer>
  </ProductProviders>;
}
