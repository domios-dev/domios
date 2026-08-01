// Run: npx tsx src/components/Canvas/taskExecutorHistory.test.ts

import type { Tab } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import {
  buildHistoricalExecutorView,
  historicalExecutorForTask,
  resolveTaskExecutionAttempts,
} from "./taskExecutorHistory";
import { buildTaskPresence } from "./taskPresence";
import { resolvePlanExclusiveTabTasks } from "./tabCurrentTask";
import { TaskStatus, type Plan, type Task, type TaskExecutionAttempt } from "./types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const tabs: Record<string, Tab> = {
  "tab-42": {
    id: "tab-42",
    pty_id: null,
    tab_type: "Terminal",
    title: "Puccini",
    cwd: "/tmp",
    agent_type: "codex",
  },
};

const activeTask: Task = {
  ...FIXTURE_TASK,
  status: TaskStatus.InProgress,
  assigned_tab_id: "tab-42",
  taken_by_role: "backend-dev",
  model: "gpt-5.5",
  lane: { tab_id: "tab-42", branch: "feat/history-ui" },
};

const doneTask: Task = {
  ...activeTask,
  status: TaskStatus.Done,
  milestones: [
    {
      id: "ms-1",
      from: { tab_id: "tab-42", session_id: null, title: null },
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
const exclusive = resolvePlanExclusiveTabTasks(plan);

const activePresence = buildTaskPresence(plan, activeTask, tabs, exclusive);
assert(activePresence?.label === "Puccini", "active task has live presence");
assert(historicalExecutorForTask(activeTask, tabs, true) === null, "active presence suppresses historical");

const attempts = resolveTaskExecutionAttempts(doneTask, tabs);
assert(attempts.length === 1, "legacy done task synthesizes one attempt");
assert(attempts[0].tab_id === "tab-42", "attempt keeps Tab id");
assert(attempts[0].model === "gpt-5.5", "attempt keeps model");

const historical = buildHistoricalExecutorView(doneTask, tabs);
assert(historical?.label === "Puccini", `historical label=${historical?.label}`);
assert(historical?.summary.includes("Puccini"), "summary includes Tab title");
assert(historical?.summary.includes("backend-dev"), "summary includes role");
assert(historical?.summary.includes("gpt-5.5"), "summary includes model");

assert(historicalExecutorForTask(doneTask, tabs, false)?.summary === historical?.summary, "terminal task exposes historical");
assert(historicalExecutorForTask(doneTask, tabs, true) === null, "historical hidden when active presence flag set");

const storedAttempts: TaskExecutionAttempt[] = [
  {
    tab_id: "tab-a",
    tab_title: "Worker A",
    model: "claude-sonnet",
    role: "frontend-dev",
    branch: "feat/a",
    report_status: "done",
    outcome: "done",
  },
  {
    tab_id: "tab-b",
    tab_title: "Worker B",
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
const multi = buildHistoricalExecutorView(multiTask, tabs);
assert(multi?.attemptCount === 2, "stored attempts preserved");
assert(multi?.label === "Worker B", "latest attempt leads summary");
assert(multi?.summary.includes("partial"), "latest report status in summary");

const rustFieldsTask: Task = {
  ...doneTask,
  assigned_tab_id: null,
  lane: {},
  taken_by_role: "worker",
  execution_attempts: [
    {
      id: "att-1",
      executor_tab_id: "tab-42",
      executor_title: "Puccini",
      role: "worker",
      model: "gpt-5.5",
      closed_at: 99,
      report: { status: "done" },
    },
  ],
};
const rustHistorical = buildHistoricalExecutorView(rustFieldsTask, tabs);
assert(rustHistorical?.label === "Puccini", `rust field label=${rustHistorical?.label}`);
assert(!rustHistorical?.summary.toLowerCase().includes("unknown"), "rust fields avoid unknown executor");

console.log("taskExecutorHistory: all assertions passed");
