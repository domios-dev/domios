// Run: npx tsx src/components/Canvas/planWorkspaceScope.test.ts

import type { Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { derivePlanWorkspaceScope } from "./planWorkspaceScope";
import type { Plan } from "./types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const WORKSPACES: Workspace[] = [
  { id: "ws-owner", name: "Canvas Hub", tabs: ["orch-1"], active_tab_id: null, cwd: "/repo/hub" },
  { id: "ws-api", name: "API Lane", tabs: ["tab-api"], active_tab_id: null, cwd: "/repo/api" },
  { id: "ws-ui", name: "UI Lane", tabs: ["tab-ui"], active_tab_id: null, cwd: "/repo/ui" },
];

test("derivePlanWorkspaceScope identifies owner workspace from owner Tab", () => {
  const scope = derivePlanWorkspaceScope(FIXTURE_PLAN, WORKSPACES);
  assert(scope.ownerWorkspaceLabel === "Canvas Hub", `owner workspace label, got ${scope.ownerWorkspaceLabel}`);
  assert(scope.isCrossWorkspace === false, "single owner workspace is not cross-workspace");
  assert(scope.participantWorkspaceLabels.length === 1, "owner workspace participates");
});

test("derivePlanWorkspaceScope marks cross-workspace when task lanes span workspaces", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "task-api", name: "API", assigned_tab_id: "tab-api" },
      {
        ...FIXTURE_TASK,
        id: "task-ui",
        name: "UI",
        assigned_tab_id: "tab-ui",
        lane: { tab_id: "tab-ui", workspace_id: "ws-ui", workspace_name: "UI Lane" },
      },
    ],
  };

  const scope = derivePlanWorkspaceScope(plan, WORKSPACES);
  assert(scope.isCrossWorkspace === true, "multiple task workspaces make plan cross-workspace");
  assert(scope.ownerWorkspaceLabel === "Canvas Hub", "owner workspace remains the owner label");
  assert(scope.participantWorkspaceLabels.includes("API Lane"), "assigned Tab workspace included");
  assert(scope.participantWorkspaceLabels.includes("UI Lane"), "lane workspace included");
});

test("derivePlanWorkspaceScope falls back to lane metadata when workspace tabs are unavailable", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    owner: { ...FIXTURE_PLAN.owner, tab_id: "orch-missing" },
    tasks: [
      {
        ...FIXTURE_TASK,
        id: "task-owned",
        lane: { tab_id: "orch-missing", workspace_id: "ws-owner", workspace_name: "Owner Lane" },
      },
      {
        ...FIXTURE_TASK,
        id: "task-remote",
        lane: { tab_id: "tab-remote", workspace_id: "ws-remote", workspace_name: "Remote Lane" },
      },
    ],
  };

  const scope = derivePlanWorkspaceScope(plan, []);
  assert(scope.ownerWorkspaceLabel === "Owner Lane", `owner fallback from lane, got ${scope.ownerWorkspaceLabel}`);
  assert(scope.isCrossWorkspace === true, "lane metadata alone can mark cross-workspace");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}`);
    console.error(`    ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`\n${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
