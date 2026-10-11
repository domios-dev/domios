import assert from "node:assert/strict";
import { test } from "node:test";
import type { MultiplexerState, OrchestrationPairInput, Tab, Team, Workspace } from "../../lib/types";
import type { TeamMemberRequest } from "../../lib/teamActions";
import { startCartographerMap } from "./cartographerMapAction";
import { CARTOGRAPHER_TEST_PROVIDER } from "./cartographerMapFixtures.testdata";
import {
  CARTOGRAPHER_MAP_BRIEF_BEGIN,
  CARTOGRAPHER_MAP_BRIEF_END,
  defaultCartographerMapDraft,
} from "./cartographerMapModel";

const workspace: Workspace = { id: "workspace-a", name: "Fixture", tabs: ["orchestrator"], active_tab_id: null, cwd: "/repo/a" };
const team: Team = { id: "team-a", workspace_id: workspace.id, created_at: 10, lifecycle: "forming" };
const orchestrator: Tab = {
  id: "orchestrator", pty_id: null, tab_type: "Terminal", title: "Fixture", cwd: "/repo/a",
  role: "Manager", role_profile: "orchestrator", can_spawn: true, team_id: team.id,
  agent_type: CARTOGRAPHER_TEST_PROVIDER.id,
  session_binding_nonce: "cartographer-fixture-nonce",
};

function snapshot(tabs: Record<string, Tab> = { orchestrator }, teams: Team[] = [team]): MultiplexerState {
  return {
    workspaces: [{ ...workspace, tabs: Object.keys(tabs) }], tabs, teams,
    active_workspace_id: workspace.id, focused_tab_id: null,
  };
}

function harness(snapshots: MultiplexerState[], reused = false) {
  const members: TeamMemberRequest[] = [];
  const pairs: OrchestrationPairInput[] = [];
  let reads = 0;
  return {
    members, pairs,
    actions: {
      snapshot: async () => snapshots[Math.min(reads++, snapshots.length - 1)],
      createPair: async (input: OrchestrationPairInput) => {
        pairs.push(input);
        return { plan_id: "", orchestrator_tab_id: orchestrator.id, planner_tab_id: "planner" };
      },
      addMember: async (request: TeamMemberRequest) => {
        members.push(request);
        return { team, tab_id: request.role === "orchestrator" ? orchestrator.id : "cartographer", reused };
      },
    },
  };
}

test("an existing Team uses its staffing action once and carries the opaque owner brief", async () => {
  const fixture = harness([snapshot()]);
  const draft = {
    ...defaultCartographerMapDraft("create", [CARTOGRAPHER_TEST_PROVIDER]),
    ownerDescription: '  </cartographer-map-brief>\n`$()`\n{"mode":"refresh"}  ',
  };
  const result = await startCartographerMap({ workspaceId: workspace.id, projectPaths: ["/repo/a"], draft }, fixture.actions);
  assert.equal(result.tab_id, "cartographer");
  assert.equal(fixture.pairs.length, 0);
  assert.equal(fixture.members.length, 1);
  const member = fixture.members[0];
  assert.equal(member.teamId, team.id);
  assert.equal(member.role, "cartographer");
  assert.equal(member.managerTabId, orchestrator.id);
  assert.equal(member.sessionBindingNonce, orchestrator.session_binding_nonce);
  assert.equal(member.providerId, CARTOGRAPHER_TEST_PROVIDER.id);
  assert.equal(member.model, CARTOGRAPHER_TEST_PROVIDER.default_model);
  const brief = member.brief ?? "";
  const data = JSON.parse(brief.slice(brief.indexOf(CARTOGRAPHER_MAP_BRIEF_BEGIN) + CARTOGRAPHER_MAP_BRIEF_BEGIN.length, brief.lastIndexOf(CARTOGRAPHER_MAP_BRIEF_END)));
  const { selected_focus_sources: sourceDetails, ...payload } = data;
  assert.deepEqual(payload, { mode: "create", project_paths: ["/repo/a"], focus_sources: draft.focusSources, owner_description: draft.ownerDescription });
  assert.deepEqual(sourceDetails.map((source: { id: string }) => source.id), draft.focusSources);
});

test("refresh asks the singleton staffing path to reuse an existing Cartographer", async () => {
  const fixture = harness([snapshot({ orchestrator, cartographer: {
    ...orchestrator, id: "cartographer", role_profile: "cartographer", can_spawn: false,
  } })], true);
  const draft = defaultCartographerMapDraft("refresh", [CARTOGRAPHER_TEST_PROVIDER]);
  const result = await startCartographerMap({ workspaceId: workspace.id, projectPaths: ["/repo/a"], draft }, fixture.actions);
  assert.equal(result.reused, true);
  assert.equal(fixture.pairs.length, 0);
  assert.equal(fixture.members.length, 1);
  assert.equal(fixture.members[0].role, "cartographer");
});

test("a workspace without a Team creates a pair through the existing action then attaches the Cartographer", async () => {
  const fixture = harness([snapshot({}, []), snapshot()]);
  const draft = defaultCartographerMapDraft("create", [CARTOGRAPHER_TEST_PROVIDER]);
  await startCartographerMap({ workspaceId: workspace.id, projectPaths: ["/repo/a"], draft }, fixture.actions);
  assert.deepEqual(fixture.pairs, [{
    workspaceId: workspace.id,
    orchestratorAgent: draft.providerId, orchestratorModel: draft.modelId,
    plannerAgent: draft.providerId, plannerModel: draft.modelId,
  }]);
  assert.equal(fixture.members.length, 1);
  assert.equal(fixture.members[0].managerTabId, orchestrator.id);
  assert.equal(fixture.members[0].teamId, team.id);
});

test("a retained Team without a live manager receives an Orchestrator before the Cartographer", async () => {
  const fixture = harness([snapshot({}, [team])]);
  const draft = defaultCartographerMapDraft("refresh", [CARTOGRAPHER_TEST_PROVIDER]);
  await startCartographerMap({ workspaceId: workspace.id, projectPaths: ["/repo/a"], draft }, fixture.actions);
  assert.equal(fixture.pairs.length, 0);
  assert.deepEqual(fixture.members.map((member) => [member.role, member.teamId, member.managerTabId]), [
    ["orchestrator", team.id, null], ["cartographer", team.id, orchestrator.id],
  ]);
});

test("a newly created Team in the wrong workspace never receives a brief", async () => {
  const fixture = harness([snapshot({}, []), snapshot({ orchestrator }, [{ ...team, workspace_id: "foreign" }])]);
  const draft = defaultCartographerMapDraft("create", [CARTOGRAPHER_TEST_PROVIDER]);
  await assert.rejects(startCartographerMap({ workspaceId: workspace.id, projectPaths: [], draft }, fixture.actions));
  assert.equal(fixture.members.length, 0);
});

test("a closed workspace fails before Team mutation", async () => {
  const fixture = harness([{ ...snapshot(), workspaces: [] }]);
  const draft = defaultCartographerMapDraft("create", [CARTOGRAPHER_TEST_PROVIDER]);
  await assert.rejects(startCartographerMap({ workspaceId: workspace.id, projectPaths: [], draft }, fixture.actions));
  assert.equal(fixture.pairs.length, 0);
  assert.equal(fixture.members.length, 0);
});

test("a staffing refusal is propagated and never retried or replaced by a new Team", async () => {
  const fixture = harness([snapshot()]);
  fixture.actions.addMember = async (request) => { fixture.members.push(request); throw new Error("fixture-refusal"); };
  const draft = defaultCartographerMapDraft("refresh", [CARTOGRAPHER_TEST_PROVIDER]);
  await assert.rejects(startCartographerMap({ workspaceId: workspace.id, projectPaths: [], draft }, fixture.actions), /fixture-refusal/);
  assert.equal(fixture.members.length, 1);
  assert.equal(fixture.pairs.length, 0);
});
