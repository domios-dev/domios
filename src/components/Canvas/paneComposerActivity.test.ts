// Run: npx tsx src/components/Canvas/paneComposerActivity.test.ts

import { resolvePaneComposerActivity } from "./paneComposerActivity";
import type { Pane } from "../../lib/types";
import type { Plan, Task, TaskStatus } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const emptyCanvas = { x: null, y: null, collapsed: false };

function mkTask(id: string, status: TaskStatus, extra: Partial<Task> = {}): Task {
  return {
    id,
    name: `Task ${id}`,
    type: "implement",
    model: null,
    model_rationale: null,
    status,
    status_label: null,
    section_ids: [],
    body: "",
    intent: null,
    parent_task_id: null,
    assigned_pane_id: null,
    peer: null,
    canvas: emptyCanvas,
    milestones: [],
    ...extra,
  };
}

function mkPlan(over: Partial<Plan> = {}): Plan {
  return {
    id: "plan-1",
    title: "Canvas v2 — dogfood",
    sections: [],
    status: "active",
    owner: { pane_id: "orch-1", session_id: "sess-orch", title: "Orchestrator" },
    project_path: "/tmp/p",
    created_at: 1,
    updated_at: 100,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks: [],
    canvas: { viewport: null },
    owner_history: [],
    ...over,
  };
}

function mkPane(id: string, over: Partial<Pane> = {}): Pane {
  return { id, pty_id: null, pane_type: "Terminal", title: id, cwd: null, ...over } as Pane;
}

test("owner pane → orchestrating with plan title", () => {
  const plan = mkPlan();
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) };
  const activity = resolvePaneComposerActivity([plan], "orch-1", panes);
  assert(activity?.kind === "orchestrating", "kind orchestrating");
  assert(activity?.planTitle === "Canvas v2 — dogfood", "plan title");
  assert(activity?.extraPlans === 0, "no extra plans");
});

test("assigned worker → working with task + plan titles", () => {
  const task = mkTask("task-1", "in_progress", {
    name: "Composer plan activity context above input",
    assigned_pane_id: "worker-1",
  });
  const plan = mkPlan({ tasks: [task] });
  const activity = resolvePaneComposerActivity([plan], "worker-1", {});
  assert(activity?.kind === "working", "kind working");
  assert(activity?.taskTitle === "Composer plan activity context above input", "task title");
  assert(activity?.planTitle === "Canvas v2 — dogfood", "plan title for secondary");
});

test("owner wins over own task assignment", () => {
  const task = mkTask("task-1", "in_progress", {
    name: "Still orchestrating",
    assigned_pane_id: "orch-1",
  });
  const plan = mkPlan({ tasks: [task] });
  const panes = { "orch-1": mkPane("orch-1", { agent_session_id: "sess-orch" }) };
  const activity = resolvePaneComposerActivity([plan], "orch-1", panes);
  assert(activity?.kind === "orchestrating", "orchestrator label beats worker task");
});

test("unrelated pane → null", () => {
  const plan = mkPlan({ tasks: [mkTask("task-1", "in_progress", { assigned_pane_id: "worker-1" })] });
  assert(resolvePaneComposerActivity([plan], "stranger", {}) === null, "no activity");
});

test("task handle fallback when name empty", () => {
  const task = mkTask("task-1", "in_progress", {
    name: "  ",
    handle: "composer-bubble-alignment",
    assigned_pane_id: "worker-1",
  });
  const plan = mkPlan({ tasks: [task] });
  const activity = resolvePaneComposerActivity([plan], "worker-1", {});
  assert(activity?.kind === "working", "kind working");
  assert(activity.taskTitle === "composer-bubble-alignment", "handle not raw id");
});

async function run() {
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n${tests.length} passed`);
}

void run();
