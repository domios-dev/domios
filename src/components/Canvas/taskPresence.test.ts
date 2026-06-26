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
  taken_by_role: "backend-dev",
  peer: "claude-sonnet, gpt-5-codex",
  lane: { branch: "feat/login-endpoint" },
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
assert(presence?.branch === "feat/login-endpoint", `branch surfaced, got ${presence?.branch}`);
assert(presence?.agents.length === 3, `lead + 2 peers resolved, got ${presence?.agents.length}`);
assert(presence?.agents[0].label === "Puccini", `lead prefers pane title, got ${presence?.agents[0].label}`);
assert(presence?.agents[0].role === "backend-dev", `lead carries role, got ${presence?.agents[0].role}`);
assert(presence?.agents[1].isPeer === true, "comma-list peer flagged as peer");
assert(presence?.agents[1].label === "claude-sonnet", `first peer parsed, got ${presence?.agents[1].label}`);
assert(presence?.agents[2].label === "gpt-5-codex", `second peer parsed, got ${presence?.agents[2].label}`);
assert(presence?.agents[0].color !== presence?.agents[1].color, "distinct agents get distinct colors");

const roleFallback = buildTaskPresence(plan, { ...task, assigned_pane_id: "pane-headless" }, {});
assert(roleFallback?.agents[0].label === "backend-dev", `headless lead falls back to role handle, got ${roleFallback?.agents[0].label}`);

const unassigned = buildTaskPresence(plan, { ...FIXTURE_TASK, assigned_pane_id: null, lane: undefined }, panes);
assert(unassigned === null, "unassigned task has no live presence marker");

console.log("taskPresence: all assertions passed");
