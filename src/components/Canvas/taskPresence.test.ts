// Run: npx tsx src/components/Canvas/taskPresence.test.ts

import type { Pane } from "../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { buildTaskPresence } from "./taskPresence";
import type { Plan } from "./types";

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
};

const plan: Plan = {
  ...FIXTURE_PLAN,
  heartbeat: {
    current_focus: "Implement presence chip",
    active_lanes: ["task:task-dante"],
  },
};

const task = {
  ...FIXTURE_TASK,
  assigned_pane_id: "pane-42",
  milestones: [
    {
      ...FIXTURE_MILESTONE,
      from: { pane_id: "pane-42", session_id: null, title: null },
      kind: "status_update",
      ts: 1_700_000_200_000,
    },
  ],
};

const presence = buildTaskPresence(plan, task, panes);
assert(presence?.paneId === "pane-42", `pane id derived, got ${presence?.paneId}`);
assert(presence?.label === "Puccini", `pane title preferred, got ${presence?.label}`);
assert(presence?.focus === "Implement presence chip", `active-lane focus attached, got ${presence?.focus}`);
assert(presence?.latest?.authorLabel === "Puccini", `latest author falls back to pane title, got ${presence?.latest?.authorLabel}`);
assert(presence?.latest?.kindLabel === "status update", `kind label humanized, got ${presence?.latest?.kindLabel}`);

const unassigned = buildTaskPresence(plan, { ...FIXTURE_TASK, assigned_pane_id: null, lane: undefined }, panes);
assert(unassigned === null, "unassigned task has no live presence marker");

console.log("taskPresence: all assertions passed");
