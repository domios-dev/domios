// Run: npx tsx src/components/Sidebar/orchestrationGroupClose.test.ts

import { countBySection } from "./orchestrationSections";
import { buildHierarchySubtree } from "./HierarchyTree";
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
    planner: pane("planner", { parent_pane_id: "orch", role: "Manager" }),
    worker1: pane("worker1", { parent_pane_id: "orch", role: "Worker" }),
    unrelated: pane("unrelated"),
  };
  const members = collectOrchestrationGroupMembers("orch", panes);
  const ids = members.map((m) => m.paneId).sort();
  assertEqual(ids, ["planner", "worker1"], "only orch's own children");
});

test("collects a valid manager and worker tree", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
    planner: pane("planner", { parent_pane_id: "orch", role: "Manager" }),
    worker1: pane("worker1", { parent_pane_id: "orch", role: "Worker" }),
    worker2: pane("worker2", { parent_pane_id: "orch", role: "Worker" }),
  };
  const members = collectOrchestrationGroupMembers("orch", panes);
  const ids = members.map((m) => m.paneId).sort();
  assertEqual(ids, ["planner", "worker1", "worker2"], "valid direct children are included");
});

test("collects workers nested under a manager", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
    manager: pane("manager", { parent_pane_id: "orch", role: "Manager" }),
    worker1: pane("worker1", { parent_pane_id: "manager", role: "Worker" }),
    worker2: pane("worker2", { parent_pane_id: "worker1", role: "Worker" }),
  };
  const members = collectOrchestrationGroupMembers("orch", panes);
  const ids = members.map((m) => m.paneId);
  assertEqual(ids, ["manager", "worker1", "worker2"], "manager subtree included");
});

test("section counts match visible worker rows for a valid tree", () => {
  const panes: Record<string, Pane> = {
    orch: pane("orch", { role: "Orchestrator" }),
    planner: pane("planner", { parent_pane_id: "orch", role: "Manager" }),
    worker1: pane("worker1", { parent_pane_id: "orch", role: "Worker" }),
    worker2: pane("worker2", { parent_pane_id: "orch", role: "Worker" }),
  };
  const managerPaneIds = new Set(
    Object.values(panes)
      .filter((p) => p.parent_pane_id === "orch" && p.role === "Manager")
      .map((p) => p.id),
  );
  const workerRows = buildHierarchySubtree("orch", panes, {}, managerPaneIds);
  const counts = countBySection(collectOrchestrationGroupMembers("orch", panes));
  assertEqual(counts, { managers: 1, workers: 2 }, "section counts");
  assertEqual(workerRows.map((n) => n.pane.id).sort(), ["worker1", "worker2"], "visible worker rows");
  assertEqual(counts.workers, workerRows.length, "worker count matches rendered rows");
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
