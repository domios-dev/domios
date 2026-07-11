// Run: npx tsx src/components/Canvas/planView/activeWork.test.ts

import type { Pane } from "../../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import {
  IN_PROGRESS_ACTIVITY_STALE_MS,
  IN_REVIEW_UNJUDGED_STALE_MS,
  activeWorkForPane,
  activeWorkForPlan,
  displayTaskTitle,
  enrichSwitcherOptions,
  inflightTooltip,
  isActiveWorkStale,
  selectActiveWork,
  taskActivityFreshnessTs,
} from "./activeWork";
import { TaskStatus, type Plan, type Task } from "../types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const NOW = 1_700_000_000_000;

const panes: Record<string, Pane> = {
  "pane-1": {
    id: "pane-1",
    pty_id: "pty-1",
    pane_type: "Terminal",
    title: "Puccini",
    cwd: "/repo",
    agent_type: "codex",
  },
};

function task(over: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...over };
}

function plan(over: Partial<Plan> & { tasks?: Task[] }): Plan {
  const { tasks = [], ...rest } = over;
  return { ...FIXTURE_PLAN, ...rest, tasks };
}

const freshWip = task({
  id: "t-wip",
  status: TaskStatus.InProgress,
  name: "Fresh WIP",
  assigned_pane_id: "pane-1",
  updated_at: NOW - 60_000,
  status_changed_at: NOW - 60_000,
  milestones: [{ ...FIXTURE_MILESTONE, id: "m1", ts: NOW - 30_000 }],
});

const staleReview = task({
  id: "t-review",
  status: TaskStatus.InReview,
  name: "Stale review",
  assigned_pane_id: "pane-1",
  status_changed_at: NOW - IN_REVIEW_UNJUDGED_STALE_MS - 1,
});

const staleProgress = task({
  id: "t-stale",
  status: TaskStatus.InProgress,
  name: "Stale progress",
  assigned_pane_id: "pane-1",
  updated_at: NOW - IN_PROGRESS_ACTIVITY_STALE_MS - 1,
  status_changed_at: NOW - IN_PROGRESS_ACTIVITY_STALE_MS - 1,
  milestones: [],
});

const p = plan({ tasks: [staleReview, freshWip, staleProgress] });
const snapshot = selectActiveWork([p], panes, NOW);

assert(snapshot.entries.length === 3, `three in-flight tasks, got ${snapshot.entries.length}`);
assert(snapshot.entries[0]?.task.id === "t-wip", "fresh in_progress sorts first");
assert(snapshot.entries.some((e) => e.task.id === "t-review" && e.stale), "stale in_review flagged");
assert(snapshot.entries.some((e) => e.task.id === "t-stale" && e.stale), "stale in_progress flagged");
assert(snapshot.countByPlanId.get(p.id) === 3, "per-plan count");
assert(activeWorkForPlan(snapshot, p.id).length === 3, "plan bucket");
assert(activeWorkForPane(snapshot, "pane-1")?.task.id === "t-wip", "pane resolves freshest bound task");
assert(displayTaskTitle(task({ name: "  Dante  " })) === "Dante", "display task title");
assert(taskActivityFreshnessTs(freshWip) === NOW - 30_000, "freshness uses latest milestone");
assert(!isActiveWorkStale(freshWip, NOW), "fresh wip not stale");
assert(isActiveWorkStale(staleReview, NOW), "stale review detected");
assert(isActiveWorkStale(staleProgress, NOW), "stale progress detected");

const enriched = enrichSwitcherOptions([{ id: p.id, title: "Auth" }], snapshot)[0];
assert(enriched.inflightCount === 3, "switcher enrichment count");
assert(enriched.inflightTooltip?.includes("Fresh WIP"), "switcher tooltip lists tasks");
assert(inflightTooltip(snapshot.entries).includes("Puccini"), "tooltip includes executor");

const noPane = selectActiveWork([plan({ tasks: [task({ id: "x", status: TaskStatus.InReview, assigned_pane_id: "gone" })] })], panes, NOW);
assert(noPane.entries[0]?.executorPresent === false, "missing pane → no presence");

const paused = selectActiveWork([plan({ mode: "paused", tasks: [freshWip] })], panes, NOW);
assert(paused.entries.length === 0, "paused plans do not contribute active work");

console.log("activeWork: all assertions passed");
