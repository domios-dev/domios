// Run: npx tsx src/components/Canvas/canvasFocusPayload.test.ts

import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { resolveCanvasFocusNodeIds } from "./canvasFocusPayload";
import { TaskStatus, type Plan } from "./types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const planWithTabTask: Plan = {
  ...FIXTURE_PLAN,
  tasks: [{ ...FIXTURE_TASK, assigned_tab_id: "tab-worker" }],
};

test("task_id from POST /api/canvas/focus resolves to a focus target", () => {
  const ids = resolveCanvasFocusNodeIds({ task_id: "task-dante" }, [FIXTURE_PLAN]);
  assert(ids.length === 1 && ids[0] === "task-dante", `expected task-dante, got ${ids.join()}`);
});

test("plan_id resolves to the plan node id", () => {
  const ids = resolveCanvasFocusNodeIds({ plan_id: FIXTURE_PLAN.id }, [FIXTURE_PLAN]);
  assert(ids.length === 1 && ids[0] === FIXTURE_PLAN.id, `expected plan id, got ${ids.join()}`);
});

test("legacy node_ids and plan_ids still work", () => {
  const ids = resolveCanvasFocusNodeIds(
    { node_ids: ["n1"], plan_ids: ["plan-beta"] },
    [FIXTURE_PLAN],
  );
  assert(ids.join() === "n1,plan-beta", `legacy ids merged, got ${ids.join()}`);
});

test("tab_id resolves to tasks assigned to that tab", () => {
  const ids = resolveCanvasFocusNodeIds({ tab_id: "tab-worker" }, [planWithTabTask]);
  assert(ids.length === 1 && ids[0] === "task-dante", `Tab assignment, got ${ids.join()}`);
});

test("tab_id resolves trimmed active bindings and ignores blank payloads", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [{ ...FIXTURE_TASK, assigned_tab_id: " tab-worker " }],
  };
  assert(resolveCanvasFocusNodeIds({ tab_id: " tab-worker " }, [plan]).join() === "task-dante", "trimmed Tab binding");
  assert(resolveCanvasFocusNodeIds({ tab_id: " " }, [plan]).length === 0, "blank Tab payload ignored");
});

test("tab_id ignores terminal stale bindings", () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "done", status: TaskStatus.Done, assigned_tab_id: "tab-worker" },
      { ...FIXTURE_TASK, id: "cancelled", status: TaskStatus.Cancelled, assigned_tab_id: "tab-worker" },
    ],
  };
  const ids = resolveCanvasFocusNodeIds({ tab_id: "tab-worker" }, [plan]);
  assert(ids.length === 0, `terminal assignments ignored, got ${ids.join()}`);
});

test("null fields are ignored", () => {
  const ids = resolveCanvasFocusNodeIds(
    { task_id: null, plan_id: null, tab_id: null },
    [FIXTURE_PLAN],
  );
  assert(ids.length === 0, `expected empty, got ${ids.join()}`);
});

test("task_id combines with legacy node_ids", () => {
  const ids = resolveCanvasFocusNodeIds(
    { task_id: "task-dante", node_ids: ["legacy-node"] },
    [FIXTURE_PLAN],
  );
  assert(ids.join() === "legacy-node,task-dante", `combined ids, got ${ids.join()}`);
});

function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} canvasFocusPayload tests passed`);
}

main();
