// Run: npx tsx src/components/Canvas/planPaneMatch.test.ts

import type { Pane } from "../../lib/types";
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
  const out = filterPlansForCanvas([FIXTURE_PLAN, other], "focus", { planId: FIXTURE_PLAN.id });
  assert(out.length === 1 && out[0].id === FIXTURE_PLAN.id, "single plan focus");
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
