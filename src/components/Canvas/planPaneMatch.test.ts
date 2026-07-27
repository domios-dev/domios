// Run: npx tsx src/components/Canvas/planPaneMatch.test.ts

import type { Pane, Team, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import {
  collectPaneChain,
  findPlansForPane,
  filterPlansForCanvas,
  resolveOrchestratorPaneId,
} from "./planPaneMatch";
import type { Plan } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const orchPane: Pane = {
  id: "orch-1",
  pty_id: "pty-1",
  pane_type: "Terminal",
  title: "Orchestrator",
  cwd: null,
  role: "Orchestrator",
  can_spawn: true,
  agent_session_id: "sess-1",
};

const workerPane: Pane = {
  id: "worker-1",
  pty_id: "pty-2",
  pane_type: "Terminal",
  title: "Worker",
  cwd: null,
  role: "Worker",
  parent_pane_id: "orch-1",
};

const panes: Record<string, Pane> = {
  "orch-1": orchPane,
  "worker-1": workerPane,
};

const assignedPlan: Plan = {
  ...FIXTURE_PLAN,
  tasks: [{ ...FIXTURE_TASK, assigned_pane_id: "worker-1" }],
};

test("findPlansForPane matches owner orchestrator", () => {
  const found = findPlansForPane([FIXTURE_PLAN], "orch-1", panes);
  assert(found.length === 1, "orchestrator sees owned plan");
  assert(found[0].id === FIXTURE_PLAN.id, "correct plan id");
});

test("findPlansForPane matches worker task binding", () => {
  const found = findPlansForPane([assignedPlan], "worker-1", panes);
  assert(found.length === 1, "worker sees assigned plan");
});

test("findPlansForPane ignores archived plans", () => {
  const archived = { ...FIXTURE_PLAN, status: "archived" as const };
  const found = findPlansForPane([archived], "orch-1", panes);
  assert(found.length === 0, "archived hidden");
});

test("filterPlansForCanvas focus by planId", () => {
  const other = { ...FIXTURE_PLAN, id: "plan-beta", title: "Beta" };
  const out = filterPlansForCanvas([FIXTURE_PLAN, other], "focus", { planId: FIXTURE_PLAN.id, workspaceId: undefined });
  assert(out.length === 1 && out[0].id === FIXTURE_PLAN.id, "single plan focus");
});

function workspace(id: string, paneIds: string[]): Workspace {
  return {
    id,
    name: id,
    tabs: paneIds.map((pane_id, i) => ({ id: `${id}-pane-${i}`, pane_id })),
    active_tab_id: null,
    cwd: null,
  };
}

const WS_CURRENT = "ws-current";
const WS_OTHER = "ws-other";
// FIXTURE_PLAN.owner.pane_id is orch-1 → lives in the current workspace.
const scopeWorkspaces: Workspace[] = [
  workspace(WS_CURRENT, ["orch-1"]),
  workspace(WS_OTHER, ["orch-2"]),
];
const scopeTeams: Team[] = [
  { id: "team-other", lifecycle: "active", created_at: 0, workspace_id: WS_OTHER },
  { id: "team-current", lifecycle: "active", created_at: 0, workspace_id: WS_CURRENT },
];

test("workspace mode excludes an unbound active plan (no team, owner Tab not in workspace)", () => {
  const unbound: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-unbound",
    title: "Soak load plan",
    owner: { pane_id: "ghost-pane", session_id: null, title: "Ghost" },
    tasks: [],
  };
  const out = filterPlansForCanvas([unbound], "workspace", {
    workspaceId: WS_CURRENT,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(out.length === 0, `unbound draft must not leak into workspace board (got ${out.length})`);
});

test("workspace mode excludes a team-bound plan of another workspace", () => {
  const foreign: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-foreign",
    team_id: "team-other",
    owner: { pane_id: "ghost-pane", session_id: null, title: "Ghost" },
    tasks: [],
  };
  const out = filterPlansForCanvas([foreign], "workspace", {
    workspaceId: WS_CURRENT,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(out.length === 0, `another workspace's team plan must not appear (got ${out.length})`);
});

test("workspace mode with a missing workspaceId returns [] (never every plan)", () => {
  const out = filterPlansForCanvas([{ ...FIXTURE_PLAN }], "workspace", {
    workspaceId: undefined,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(out.length === 0, `unscoped workspace mode must not leak plans (got ${out.length})`);
});

test("workspace mode keeps plans bound via owner Tab or team", () => {
  const ownerBound = { ...FIXTURE_PLAN, id: "plan-owner" }; // owner orch-1 ∈ current ws
  const teamBound: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-team",
    team_id: "team-current",
    owner: { pane_id: "ghost-pane", session_id: null, title: "Ghost" },
  };
  const out = filterPlansForCanvas([ownerBound, teamBound], "workspace", {
    workspaceId: WS_CURRENT,
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(out.length === 2, `both workspace-bound plans kept (got ${out.length})`);
});

test("Team canvas cannot escape to another plan", () => {
  const teamPlan: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-team-current",
    team_id: "team-current",
    owner: { pane_id: "ghost-pane", session_id: null, title: "Ghost" },
  };
  const foreignPlan: Plan = { ...FIXTURE_PLAN, id: "plan-foreign", team_id: "team-other" };
  const out = filterPlansForCanvas([teamPlan, foreignPlan], "workspace", {
    workspaceId: WS_CURRENT,
    teamId: "team-current",
    workspaces: scopeWorkspaces,
    teams: scopeTeams,
  });
  assert(out.length === 1 && out[0].id === teamPlan.id, "Team canvas stays on its Team plan");
});

test("collectPaneChain walks parent_pane_id", () => {
  const chain = collectPaneChain("worker-1", panes);
  assert(chain.length === 2, "worker + parent");
  assert(chain[0].id === "worker-1", "starts at worker");
});

test("resolveOrchestratorPaneId finds ancestor orchestrator", () => {
  assert(resolveOrchestratorPaneId("worker-1", panes) === "orch-1", "ancestor orchestrator");
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} planPaneMatch tests passed`);
}

main();
