// Run: npx tsx src/components/Canvas/planTabMatch.test.ts

import type { Tab, Team, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import {
  collectTabChain,
  findPlansForTab,
  filterPlansForCanvas,
  resolveOrchestratorTabId,
} from "./planTabMatch";
import type { Plan } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const orchTab: Tab = {
  id: "orch-1",
  pty_id: "pty-1",
  tab_type: "Terminal",
  title: "Orchestrator",
  cwd: null,
  role: "Manager", role_profile: "orchestrator",
  can_spawn: true,
  agent_session_id: "sess-1",
};

const workerTab: Tab = {
  id: "worker-1",
  pty_id: "pty-2",
  tab_type: "Terminal",
  title: "Worker",
  cwd: null,
  role: "Worker",
  parent_tab_id: "orch-1",
};

const tabs: Record<string, Tab> = {
  "orch-1": orchTab,
  "worker-1": workerTab,
};

const assignedPlan: Plan = {
  ...FIXTURE_PLAN,
  tasks: [{ ...FIXTURE_TASK, assigned_tab_id: "worker-1" }],
};

test("findPlansForTab matches owner orchestrator", () => {
  const found = findPlansForTab([FIXTURE_PLAN], "orch-1", tabs);
  assert(found.length === 1, "orchestrator sees owned plan");
  assert(found[0].id === FIXTURE_PLAN.id, "correct plan id");
});

test("findPlansForTab matches worker task binding", () => {
  const found = findPlansForTab([assignedPlan], "worker-1", tabs);
  assert(found.length === 1, "worker sees assigned plan");
});

test("findPlansForTab ignores archived plans", () => {
  const archived = { ...FIXTURE_PLAN, status: "archived" as const };
  const found = findPlansForTab([archived], "orch-1", tabs);
  assert(found.length === 0, "archived hidden");
});

test("filterPlansForCanvas focus by planId", () => {
  const other = { ...FIXTURE_PLAN, id: "plan-beta", title: "Beta" };
  const out = filterPlansForCanvas([FIXTURE_PLAN, other], "focus", { planId: FIXTURE_PLAN.id, workspaceId: undefined });
  assert(out.length === 1 && out[0].id === FIXTURE_PLAN.id, "single plan focus");
});

function workspace(id: string, tabIds: string[]): Workspace {
  return {
    id,
    name: id,
    tabs: tabIds,
    active_tab_id: null,
    cwd: null,
  };
}

const WS_CURRENT = "ws-current";
const WS_OTHER = "ws-other";
// FIXTURE_PLAN.owner.tab_id is orch-1 → lives in the current workspace.
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
    owner: { tab_id: "ghost-tab", session_id: null, title: "Ghost" },
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
    owner: { tab_id: "ghost-tab", session_id: null, title: "Ghost" },
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
    owner: { tab_id: "ghost-tab", session_id: null, title: "Ghost" },
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
    owner: { tab_id: "ghost-tab", session_id: null, title: "Ghost" },
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

test("collectTabChain walks parent_tab_id", () => {
  const chain = collectTabChain("worker-1", tabs);
  assert(chain.length === 2, "worker + parent");
  assert(chain[0].id === "worker-1", "starts at worker");
});

test("resolveOrchestratorTabId finds ancestor orchestrator", () => {
  assert(resolveOrchestratorTabId("worker-1", tabs) === "orch-1", "ancestor orchestrator");
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
  console.log(`\n✓ ${passed}/${tests.length} planTabMatch tests passed`);
}

main();
