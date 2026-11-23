import "./daemon";
import { SidebarWorkspaceTabs } from "../../../../src/components/Sidebar/SidebarWorkspaceTabs";
import { SidebarDnd } from "../../../../src/components/Sidebar/sortable";
import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { Tab, TabStatus, Team } from "../../../../src/lib/types";
import { CLAUDE, CODEX, ProductProviders } from "../../gallery/fixtures";
import { FILM_NOW } from "./clock";

export const TEAM_ID = "film-team";
export const FILM_WORKSPACE = { id: "ws-fieldnote", name: "Fieldnote" };
export const TEAM: Team = { id: TEAM_ID, name: "Offline sync", lifecycle: "active", created_at: FILM_NOW, workspace_id: FILM_WORKSPACE.id };

const ORCHESTRATOR_ID = "a41c09e2";

type Member = Omit<Partial<Tab>, "id" | "title"> & { id: string; title: string; status_line?: string };

function member({ status_line, ...fields }: Member): Tab {
  const working = status_line !== undefined;
  return {
    pty_id: `pty-${fields.id}`, tab_type: "Terminal", cwd: "C:/Developer/fieldnote", role: "Worker", role_profile: null, parent_tab_id: ORCHESTRATOR_ID,
    team_id: TEAM_ID, workspace_id: FILM_WORKSPACE.id, agent_type: CLAUDE.id, agent_session_id: `session-${fields.id}`, can_spawn: fields.role_profile === "orchestrator",
    agent_state: working ? { state: "working", status_line } : { state: "idle", status_line: "Churned for 4m 12s" },
    ...fields,
  } as Tab;
}

export const MEMBERS: Tab[] = [
  member({ id: ORCHESTRATOR_ID, title: "Helm", role: "Manager", role_profile: "orchestrator", parent_tab_id: null, agent_session_model: "opus", status_line: "Reviewing Mira's report… (3m 41s · ↓ 18.2k tokens)" }),
  member({ id: "5d7e3b10", title: "Atlas", role: "Manager", role_profile: "planner", parent_tab_id: null, agent_session_model: "fable" }),
  member({ id: "c2f86a4d", title: "Ledger", role: "Manager", role_profile: "curator", parent_tab_id: null, agent_session_model: "haiku", status_line: "Marking FN-201 done with its proof… (48s)" }),
  member({ id: "7b3e91f5", title: "Mira", agent_session_model: "opus", status_line: "Merging checklist reorders… (12m 05s · ↓ 41.7k tokens)" }),
  member({ id: "e08d4c37", title: "Orbit", agent_type: CODEX.id, agent_session_model: "gpt-6-sol", status_line: "Sync QA matrix on gpu-rig, 31 of 48 (6m 22s)" }),
  member({ id: "94a2f6b8", title: "Quill", agent_session_model: "sonnet", status_line: "Writing the offline sync guide… (1m 54s · ↑ 3.2k tokens)" }),
];

export const MEMBER_STATUS: Record<string, TabStatus> = Object.fromEntries(MEMBERS.filter((tab) => tab.agent_state?.state === "working").map((tab) => [tab.id, "Working" as TabStatus]));

const noop = () => undefined;

export function TeamSidebar({ plan, planIndex, members = MEMBERS }: { plan: Plan; planIndex: PlanIndexEntry; members?: Tab[] }) {
  const tabs = Object.fromEntries(members.map((tab) => [tab.id, tab]));
  return <ProductProviders tabs={members} workspaceId={FILM_WORKSPACE.id}>
    <SidebarDnd handlers={{ reorderTabs: noop, reorderDisplay: noop, reorderWorkspaces: noop }}>
      <SidebarWorkspaceTabs
        workspace={{ id: FILM_WORKSPACE.id, name: FILM_WORKSPACE.name, tabs: members.map((tab) => tab.id), active_tab_id: ORCHESTRATOR_ID, cwd: "C:/Developer/fieldnote" }}
        activeTabId={ORCHESTRATOR_ID}
        sortedTabs={members}
        tabs={tabs}
        tabStatuses={MEMBER_STATUS}
        teams={[TEAM]}
        planIndex={[planIndex]}
        plans={[plan]}
        providers={[CLAUDE, CODEX]}
        closingTabIds={new Set()}
        appearingTabIds={new Set()}
        isActiveVisual
        workspaceCanvasActive={false}
        touchMode={false}
        compact={false}
        sidebarActionText="text-xs"
        orchestrators={members.filter((tab) => tab.role_profile === "orchestrator")}
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
  </ProductProviders>;
}
