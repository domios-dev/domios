import { SidebarWorkspaceTabs } from "../../../../src/components/Sidebar/SidebarWorkspaceTabs";
import { plan, planIndexEntry, task, TEAM, tab } from "../../../../src/components/Sidebar/SidebarWorkspaceTabs.fixtures.testdata";
import { SidebarDnd } from "../../../../src/components/Sidebar/sortable";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { CLAUDE, CODEX, ProductProviders } from "../fixtures";
import { PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

type Counts = { pending?: number; in_progress?: number; in_review?: number; blocked?: number; done?: number };

function tasks(counts: Counts) {
  return Object.entries(counts).flatMap(([status, count]) => Array.from({ length: count ?? 0 }, (_, index) => task(`${status}-${index}`, status as Plan["tasks"][number]["status"])));
}

const members: Tab[] = [
  tab({ id: "team-orch", title: "Motion Design Lead", role: "Manager", role_profile: "orchestrator", team_id: TEAM.id, agent_type: CLAUDE.id, agent_session_model: "opus" }),
  tab({ id: "team-planner", title: "Release planner", role: "Manager", role_profile: "planner", team_id: TEAM.id, agent_type: CLAUDE.id, agent_session_model: "fable" }),
  tab({ id: "team-worker-a", title: "Gallery worker", role: "Worker", team_id: TEAM.id, parent_tab_id: "team-orch", agent_type: CODEX.id, agent_session_model: "gpt-6-sol" }),
  tab({ id: "team-worker-b", title: "Docs worker", role: "Worker", team_id: TEAM.id, parent_tab_id: "team-orch", agent_type: CLAUDE.id, agent_session_model: "sonnet" }),
];

type TeamSpecimenProps = { counts?: Counts; working?: boolean; menu?: boolean; submenu?: string };

const noop = () => undefined;

function TeamSpecimen({ counts, working = false, menu = false, submenu }: TeamSpecimenProps) {
  const tabs = Object.fromEntries(members.map((member) => [member.id, working && member.role === "Worker" ? { ...member, agent_state: { state: "working" as const } } : member]));
  const statuses: Record<string, TabStatus> = working ? { "team-worker-a": "Working", "team-worker-b": "Working" } : {};
  const plans = counts ? [plan(tasks(counts))] : [];
  const sidebar = <div className="gallery-sidebar">
    <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
      <SidebarWorkspaceTabs
        workspace={{ id: TEAM.workspace_id, name: "CodeTerm", tabs: members.map((member) => member.id), active_tab_id: "team-orch", cwd: "/workspace" }}
        activeTabId="team-orch"
        sortedTabs={Object.values(tabs)}
        tabs={tabs}
        tabStatuses={statuses}
        teams={[TEAM]}
        planIndex={counts ? [planIndexEntry()] : []}
        plans={plans}
        activeWork={null}
        providers={[CLAUDE, CODEX]}
        closingTabIds={new Set()}
        appearingTabIds={new Set()}
        isActiveVisual
        workspaceCanvasActive={false}
        touchMode={false}
        compact={false}
        sidebarActionText="text-xs"
        orchestrators={members.filter((member) => member.role_profile === "orchestrator")}
        onBeforeOpenPlan={noop}
        onActivateTab={noop}
        onCloseTab={noop}
        onRenameTab={noop}
        onNewSession={noop}
        onRenameTeam={noop}
        onAddTeamMember={noop}
        onCloseTeam={noop}
      />
    </SidebarDnd>
  </div>;
  return <ProductProviders tabs={Object.values(tabs)}>
    {menu ? <PinnedMenu trigger='[data-testid="team-menu-trigger"]' via="click" submenu={submenu}>{sidebar}</PinnedMenu> : sidebar}
  </ProductProviders>;
}

export default defineSpecimen({
  id: "sidebar-team",
  title: "Team unit · plan entry",
  category: "Sidebar",
  where: "src/components/Sidebar/TeamUnit.tsx",
  stage: "wide",
  notes: "SidebarWorkspaceTabs renders the shipped TeamUnit: header, plan entry card with the segmented donut and status breakdown, managers and workers sections, and the team kebab menu.",
  Component: TeamSpecimen,
  states: [
    { id: "mixed", label: "Plan · mixed statuses", props: { counts: { pending: 1, in_progress: 2, done: 9 } } },
    { id: "blocked", label: "Plan · review and blocked", props: { counts: { pending: 3, in_review: 1, blocked: 1, done: 7 } } },
    { id: "all-done", label: "Plan · all done", props: { counts: { done: 12 } } },
    { id: "empty", label: "Plan · no tasks", props: { counts: {} } },
    { id: "working", label: "Workers working", props: { counts: { pending: 2, in_progress: 2, done: 4 }, working: true } },
    { id: "no-plan", label: "Team without a plan", props: {} },
    { id: "menu", label: "Team menu open", props: { counts: { pending: 1, done: 3 }, menu: true } },
    { id: "menu-add-worker", label: "Team menu · Add worker providers", props: { counts: { pending: 1, done: 3 }, menu: true, submenu: "Add worker" } },
  ],
});
