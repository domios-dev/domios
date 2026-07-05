// Run: npx tsx src/components/Sidebar/orchestrationGroupClose.test.ts

import { collectOrchestrationGroupMembers } from "./orchestrationGroupClose";
import type { Pane } from "../../lib/types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assertEqual(actual: unknown, expected: unknown, message: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`);
  }
}

function pane(id: string, overrides: Partial<Pane> = {}): Pane {
  return { id, title: id, parent_pane_id: null, role: null, ...overrides } as Pane;
}

test("collects the planner and workers under an orchestrator, but not siblings", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
    planner: pane("planner", { parent_pane_id: "orch", role: "Worker" }),
    worker1: pane("worker1", { parent_pane_id: "orch", role: "Worker" }),
    unrelated: pane("unrelated"),
  };
  const members = collectOrchestrationGroupMembers("orch", panes);
  const ids = members.map((m) => m.paneId).sort();
  assertEqual(ids, ["planner", "worker1"], "only orch's own children");
});

test("recurses into grandchildren so nothing is left orphaned", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
    planner: pane("planner", { parent_pane_id: "orch", role: "Worker" }),
    subworker: pane("subworker", { parent_pane_id: "planner", role: "Worker" }),
  };
  const members = collectOrchestrationGroupMembers("orch", panes);
  const ids = members.map((m) => m.paneId).sort();
  assertEqual(ids, ["planner", "subworker"], "descendants at any depth are included");
});

test("an orchestrator with no live children returns an empty list", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
  };
  assertEqual(collectOrchestrationGroupMembers("orch", panes), [], "no members");
});

function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`orchestrationGroupClose tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

main();
