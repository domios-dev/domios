// Run: npx tsx src/components/Canvas/taskPresence.test.ts

import type { Pane } from "../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { resolvePlanExclusivePaneTasks } from "./paneCurrentTask";
import { buildTaskPresence } from "./taskPresence";
import type { Plan, Task } from "./types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const panes: Record<string, Pane> = {
  "pane-42": {
    id: "pane-42",
    pty_id: null,
    pane_type: "Terminal",
    title: "Puccini",
    cwd: "/tmp/project",
    agent_type: "codex",
  },
  "pane-headless": {
    id: "pane-headless",
    pty_id: null,
    pane_type: "Terminal",
    title: "",
    cwd: "/tmp/project",
    agent_type: "codex",
  },
};

const plan: Plan = {
  ...FIXTURE_PLAN,
  heartbeat: {
    current_focus: "Implement presence chip",
    active_lanes: ["task:task-dante"],
  },
};

const task: Task = {
  ...FIXTURE_TASK,
  assigned_pane_id: "pane-42",
  taken_by_role: "backend-dev",
  peer: "claude-sonnet, gpt-5-codex",
  lane: { pane_id: "pane-42", branch: "feat/login-endpoint" },
  milestones: [
    {
      ...FIXTURE_MILESTONE,
      from: { pane_id: "pane-42", session_id: null, title: null },
      kind: "status_update",
      ts: 1_700_000_200_000,
    },
  ],
};

const exclusive = resolvePlanExclusivePaneTasks(plan);

const presence = buildTaskPresence(plan, task, panes, exclusive);
assert(presence?.paneId === "pane-42", `pane id derived, got ${presence?.paneId}`);
assert(presence?.label === "Puccini", `Tab title preferred, got ${presence?.label}`);
assert(presence?.focus === "Implement presence chip", `active-lane focus attached, got ${presence?.focus}`);
assert(presence?.latest?.authorLabel === "Puccini", `latest author falls back to Tab title, got ${presence?.latest?.authorLabel}`);
assert(presence?.latest?.kindLabel === "status update", `kind label humanized, got ${presence?.latest?.kindLabel}`);
assert(presence?.branch === "feat/login-endpoint", `branch surfaced, got ${presence?.branch}`);
assert(presence?.agents.length === 3, `lead + 2 peers resolved, got ${presence?.agents.length}`);
assert(presence?.agents[0].label === "Puccini", `lead prefers Tab title, got ${presence?.agents[0].label}`);
assert(presence?.agents[0].role === "backend-dev", `lead carries role, got ${presence?.agents[0].role}`);
assert(presence?.agents[1].isPeer === true, "comma-list peer flagged as peer");
assert(presence?.agents[1].label === "claude-sonnet", `first peer parsed, got ${presence?.agents[1].label}`);
assert(presence?.agents[2].label === "gpt-5-codex", `second peer parsed, got ${presence?.agents[2].label}`);
assert(presence?.agents[0].color !== presence?.agents[1].color, "distinct agents get distinct colors");

const roleFallback = buildTaskPresence(
  plan,
  { ...task, id: "task-headless", assigned_pane_id: "pane-headless", lane: { pane_id: "pane-headless", branch: "feat/x" } },
  panes,
  resolvePlanExclusivePaneTasks({ ...plan, tasks: [{ ...task, id: "task-headless", assigned_pane_id: "pane-headless", lane: { pane_id: "pane-headless", branch: "feat/x" } }] }),
);
assert(roleFallback?.agents[0].label === "backend-dev", `headless lead falls back to role handle, got ${roleFallback?.agents[0].label}`);

const unassigned = buildTaskPresence(plan, { ...FIXTURE_TASK, assigned_pane_id: null, lane: undefined }, panes);
assert(unassigned === null, "unassigned task has no live presence marker");

const closedAssigned = buildTaskPresence(
  plan,
  { ...task, status: "done", assigned_pane_id: "pane-42", lane: { pane_id: "pane-42", branch: "feat/closed" } },
  panes,
  resolvePlanExclusivePaneTasks({
    ...plan,
    tasks: [{ ...task, status: "done", assigned_pane_id: "pane-42", lane: { pane_id: "pane-42", branch: "feat/closed" } }],
  }),
);
assert(closedAssigned === null, "done task with stale Tab binding has no active presence marker");

const stalePlan: Plan = {
  ...plan,
  tasks: [
    { ...task, id: "task-wip", status: "in_progress", assigned_pane_id: "pane-42", updated_at: 20 },
    { ...task, id: "task-stale", status: "pending", assigned_pane_id: "pane-42", updated_at: 10 },
  ],
};
const staleExclusive = resolvePlanExclusivePaneTasks(stalePlan);
assert(staleExclusive.get("pane-42") === "task-wip", "in_progress task wins exclusive Tab binding");
assert(buildTaskPresence(stalePlan, stalePlan.tasks[1], panes, staleExclusive) === null, "stale bound task loses presence");

const looseFocusPlan: Plan = {
  ...plan,
  heartbeat: { current_focus: "pane-42 is debugging subtasks 0/8", active_lanes: [] },
  tasks: [task],
};
const looseExclusive = resolvePlanExclusivePaneTasks(looseFocusPlan);
assert(
  buildTaskPresence(looseFocusPlan, task, panes, looseExclusive)?.focus === null,
  "focus does not attach via loose pane-id substring in narrative",
);

console.log("taskPresence: all assertions passed");
