// Run: npx tsx src/components/Sidebar/orchestrationGroupClose.test.ts

import { countBySection } from "./orchestrationSections";
import { buildHierarchySubtree } from "./HierarchyTree";
import { collectOrchestrationGroupMembers } from "./orchestrationGroupClose";
import type { Tab } from "../../lib/types";

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

function tab(id: string, overrides: Partial<Tab> = {}): Tab {
  return { id, title: id, parent_tab_id: null, role: null, ...overrides } as Tab;
}

test("collects the planner and workers under an orchestrator, but not siblings", () => {
  const tabs: Record<string, Tab> = {
    orch: tab("orch", { role: "Manager", role_profile: "orchestrator" }),
    planner: tab("planner", { parent_tab_id: "orch", role: "Manager" }),
    worker1: tab("worker1", { parent_tab_id: "orch", role: "Worker" }),
    unrelated: tab("unrelated"),
  };
  const members = collectOrchestrationGroupMembers("orch", tabs);
  const ids = members.map((m) => m.tabId).sort();
  assertEqual(ids, ["planner", "worker1"], "only orch's own children");
});

test("collects a valid manager and worker tree", () => {
  const tabs: Record<string, Tab> = {
    orch: tab("orch", { role: "Manager", role_profile: "orchestrator" }),
    planner: tab("planner", { parent_tab_id: "orch", role: "Manager" }),
    worker1: tab("worker1", { parent_tab_id: "orch", role: "Worker" }),
    worker2: tab("worker2", { parent_tab_id: "orch", role: "Worker" }),
  };
  const members = collectOrchestrationGroupMembers("orch", tabs);
  const ids = members.map((m) => m.tabId).sort();
  assertEqual(ids, ["planner", "worker1", "worker2"], "valid direct children are included");
});

test("collects workers nested under a manager", () => {
  const tabs: Record<string, Tab> = {
    orch: tab("orch", { role: "Manager", role_profile: "orchestrator" }),
    manager: tab("manager", { parent_tab_id: "orch", role: "Manager" }),
    worker1: tab("worker1", { parent_tab_id: "manager", role: "Worker" }),
    worker2: tab("worker2", { parent_tab_id: "worker1", role: "Worker" }),
  };
  const members = collectOrchestrationGroupMembers("orch", tabs);
  const ids = members.map((m) => m.tabId);
  assertEqual(ids, ["manager", "worker1", "worker2"], "manager subtree included");
});

test("section counts match visible worker rows for a valid tree", () => {
  const tabs: Record<string, Tab> = {
    orch: tab("orch", { role: "Manager", role_profile: "orchestrator" }),
    planner: tab("planner", { parent_tab_id: "orch", role: "Manager" }),
    worker1: tab("worker1", { parent_tab_id: "orch", role: "Worker" }),
    worker2: tab("worker2", { parent_tab_id: "orch", role: "Worker" }),
  };
  const managerTabIds = new Set(
    Object.values(tabs)
      .filter((p) => p.parent_tab_id === "orch" && p.role === "Manager")
      .map((p) => p.id),
  );
  const workerRows = buildHierarchySubtree("orch", tabs, {}, managerTabIds);
  const counts = countBySection(collectOrchestrationGroupMembers("orch", tabs));
  assertEqual(counts, { managers: 1, workers: 2 }, "section counts");
  assertEqual(workerRows.map((n) => n.tab.id).sort(), ["worker1", "worker2"], "visible worker rows");
  assertEqual(counts.workers, workerRows.length, "worker count matches rendered rows");
});

test("an orchestrator with no live children returns an empty list", () => {
  const tabs: Record<string, Tab> = {
    orch: tab("orch", { role: "Manager", role_profile: "orchestrator" }),
  };
  assertEqual(collectOrchestrationGroupMembers("orch", tabs), [], "no members");
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
