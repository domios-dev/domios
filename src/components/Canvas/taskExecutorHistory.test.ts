// Run: npx tsx src/components/Canvas/taskExecutorHistory.test.ts

import type { Pane } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import {
  buildHistoricalExecutorView,
  historicalExecutorForTask,
  resolveTaskExecutionAttempts,
} from "./taskExecutorHistory";
import { buildTaskPresence } from "./taskPresence";
import { resolvePlanExclusivePaneTasks } from "./paneCurrentTask";
import { TaskStatus, type Plan, type Task, type TaskExecutionAttempt } from "./types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const panes: Record<string, Pane> = {
  "pane-42": {
    id: "pane-42",
    pty_id: null,
    pane_type: "Terminal",
    title: "Puccini",
    cwd: "/tmp",
    agent_type: "codex",
  },
};

const activeTask: Task = {
  ...FIXTURE_TASK,
  status: TaskStatus.InProgress,
  assigned_pane_id: "pane-42",
  taken_by_role: "backend-dev",
  model: "gpt-5.5",
  lane: { pane_id: "pane-42", branch: "feat/history-ui" },
};

const doneTask: Task = {
  ...activeTask,
  status: TaskStatus.Done,
  milestones: [
    {
      id: "ms-1",
      from: { pane_id: "pane-42", session_id: null, title: null },
      kind: "status_update",
      text: "Shipped",
      ts: 1,
      links: [],
      loop_id: null,
      iteration: null,
      canvas: { x: null, y: null, collapsed: false },
    },
  ],
};

const plan: Plan = { ...FIXTURE_PLAN, tasks: [activeTask, doneTask] };
const exclusive = resolvePlanExclusivePaneTasks(plan);

const activePresence = buildTaskPresence(plan, activeTask, panes, exclusive);
assert(activePresence?.label === "Puccini", "active task has live presence");
assert(historicalExecutorForTask(activeTask, panes, true) === null, "active presence suppresses historical");

const attempts = resolveTaskExecutionAttempts(doneTask, panes);
assert(attempts.length === 1, "legacy done task synthesizes one attempt");
assert(attempts[0].pane_id === "pane-42", "attempt keeps Tab id");
assert(attempts[0].model === "gpt-5.5", "attempt keeps model");

const historical = buildHistoricalExecutorView(doneTask, panes);
assert(historical?.label === "Puccini", `historical label=${historical?.label}`);
assert(historical?.summary.includes("Puccini"), "summary includes Tab title");
assert(historical?.summary.includes("backend-dev"), "summary includes role");
assert(historical?.summary.includes("gpt-5.5"), "summary includes model");

assert(historicalExecutorForTask(doneTask, panes, false)?.summary === historical?.summary, "terminal task exposes historical");
assert(historicalExecutorForTask(doneTask, panes, true) === null, "historical hidden when active presence flag set");

const storedAttempts: TaskExecutionAttempt[] = [
  {
    pane_id: "pane-a",
    pane_title: "Worker A",
    model: "claude-sonnet",
    role: "frontend-dev",
    branch: "feat/a",
    report_status: "done",
    outcome: "done",
  },
  {
    pane_id: "pane-b",
    pane_title: "Worker B",
    model: "gpt-5.5",
    role: "frontend-dev",
    branch: "feat/b",
    report_status: "partial",
    outcome: "cancelled",
  },
];
const multiTask: Task = {
  ...doneTask,
  execution_attempts: storedAttempts,
};
const multi = buildHistoricalExecutorView(multiTask, panes);
assert(multi?.attemptCount === 2, "stored attempts preserved");
assert(multi?.label === "Worker B", "latest attempt leads summary");
assert(multi?.summary.includes("partial"), "latest report status in summary");

const rustFieldsTask: Task = {
  ...doneTask,
  assigned_pane_id: null,
  lane: {},
  taken_by_role: "worker",
  execution_attempts: [
    {
      id: "att-1",
      executor_pane_id: "pane-42",
      executor_title: "Puccini",
      role: "worker",
      model: "gpt-5.5",
      closed_at: 99,
      report: { status: "done" },
    },
  ],
};
const rustHistorical = buildHistoricalExecutorView(rustFieldsTask, panes);
assert(rustHistorical?.label === "Puccini", `rust field label=${rustHistorical?.label}`);
assert(!rustHistorical?.summary.toLowerCase().includes("unknown"), "rust fields avoid unknown executor");

console.log("taskExecutorHistory: all assertions passed");
