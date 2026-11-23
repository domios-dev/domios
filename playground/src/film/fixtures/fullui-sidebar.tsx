import { ExtensionsIcon } from "../../../../src/components/Icons";
import { SidebarDisclosure } from "../../../../src/components/Sidebar/SidebarDisclosure";
import { SidebarDrawer } from "../../../../src/components/Sidebar/SidebarDrawer";
import { MemoryGraphIcon, MeshIcon, SidebarNavButton } from "../../../../src/components/Sidebar/SidebarNavButton";
import { SidebarWorkspaceDetail, type SidebarWorkspaceDetailActions, type SidebarWorkspaceDetailData } from "../../../../src/components/Sidebar/SidebarWorkspaceDetail";
import { WorkspaceCardGrid } from "../../../../src/components/Sidebar/WorkspaceCardGrid";
import { SidebarDnd } from "../../../../src/components/Sidebar/sortable";
import { LiquidFlip } from "../../../../src/components/shared/LiquidToggle";
import type { TabStatus } from "../../../../src/lib/types";
import { CLAUDE, CODEX, ProductProviders } from "../../gallery/fixtures";
import { ORCHESTRATOR, WORKSPACES, type TeamFrame } from "./fullui";
import { FILM_WORKSPACE, TEAM } from "./team";

export const SIDEBAR_WIDTH = 280;

const noop = () => undefined;
const ACTIONS = new Proxy({}, { get: () => noop }) as unknown as SidebarWorkspaceDetailActions;
const NONE = new Set<string>();
const NAV = { compact: false, touchMode: false, labelClassName: "text-[12px]" };
const SORT_IDS = WORKSPACES.map((workspace) => workspace.id);

/** Sidebar.tsx's expanded desktop tree: the WORKSPACES section with its card grid and the selected workspace's detail, then the footer. */
export function FullSidebar({ frame }: { frame: TeamFrame }) {
  const { members, statuses, plan, planIndex } = frame;
  const working = Object.values(statuses).includes("Working");
  const workspaceStatuses: Record<string, TabStatus | null> = { [FILM_WORKSPACE.id]: working ? "Working" : "Idle" };
  const data: SidebarWorkspaceDetailData = {
    tabs: Object.fromEntries(members.map((tab) => [tab.id, tab])),
    tabStatuses: statuses,
    attention: {},
    teams: [TEAM],
    planIndex: [planIndex],
    plans: [plan],
    providers: [CLAUDE, CODEX],
    providersLoading: false,
    closingTabIds: NONE,
    closingTeamIds: NONE,
    activeTabIds: { [FILM_WORKSPACE.id]: ORCHESTRATOR.id },
    activeWorkspaceId: FILM_WORKSPACE.id,
    isWorkspaceMode: true,
    workspaceCanvasActive: false,
    touchMode: false,
    compact: false,
    sidebarActionText: "text-[12px]",
    orchestrators: members.filter((tab) => tab.role_profile === "orchestrator"),
    newSessionDisabled: false,
  };
  return <ProductProviders tabs={members} workspaceId={FILM_WORKSPACE.id}>
    <SidebarDrawer phone={false} shown>
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
                <WorkspaceCardGrid workspaces={WORKSPACES} selectedWorkspaceId={FILM_WORKSPACE.id} statuses={workspaceStatuses} workingByWorkspace={{}} sortIds={SORT_IDS} onSelect={noop} onCreate={noop} />
                <SidebarWorkspaceDetail workspace={WORKSPACES[0]} status={workspaceStatuses[FILM_WORKSPACE.id]} data={data} actions={ACTIONS} />
              </div>
            </SidebarDnd>
          </div>
          <div role="navigation" aria-label="Sidebar views" className="sidebar-footer flex flex-col">
            <SidebarNavButton testId="nav-extensions" label="Plugins" icon={<ExtensionsIcon size={14} className="shrink-0" />} current={false} {...NAV} />
            <SidebarNavButton testId="nav-memory" label="Memory" icon={<MemoryGraphIcon size={14} />} current={false} {...NAV} />
            <div className="flex items-center gap-1 w-full">
              <SidebarNavButton testId="nav-mesh" label="Mesh" icon={<MeshIcon size={14} />} current={false} grow {...NAV} />
              <LiquidFlip aria-label="Mesh presence" className="shrink-0 text-[10px]" checked onCheckedChange={noop} />
            </div>
          </div>
        </aside>
      </div>
    </SidebarDrawer>
  </ProductProviders>;
}
