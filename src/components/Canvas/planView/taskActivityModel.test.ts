// Run: npx tsx src/components/Canvas/planView/taskActivityModel.test.ts

import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Milestone, Task } from "../types";
import {
  buildPlanRecentActivity,
  milestoneKindLabel,
  milestoneTimestampLabel,
  resolveMilestoneAuthor,
  sortMilestonesNewestFirst,
  visibleMilestones,
  VISIBLE_MILESTONE_COLLAPSE,
} from "./taskActivityModel";
import { formatTime } from "../../../lib/formatters";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function milestone(id: string, kind: string, ts: number, title: string | null = "Worker"): Milestone {
  return {
    ...FIXTURE_MILESTONE,
    id,
    kind,
    ts,
    text: `entry ${id}`,
    from: { tab_id: "tab-a", session_id: null, title },
  };
}

console.log("taskActivityModel:");

assert(milestoneKindLabel("status_update") === "status update", "kind label");
assert(resolveMilestoneAuthor(FIXTURE_MILESTONE, {}) === "Worker", "author from milestone title");
assert(resolveMilestoneAuthor(
  { ...FIXTURE_MILESTONE, from: { tab_id: "tab-b", session_id: null, title: null } },
  { "tab-b": { id: "tab-b", title: "Puccini" } as never },
) === "Puccini", "author from Tab title");
assert(resolveMilestoneAuthor(
  { ...FIXTURE_MILESTONE, from: { tab_id: "tab-x", session_id: null, title: null } },
  {},
) === null, "missing author stays empty");

const sorted = sortMilestonesNewestFirst([
  milestone("m1", "testing", 100),
  milestone("m2", "decision", 300),
  milestone("m3", "review", 200),
]);
assert(sorted.map((m) => m.id).join(",") === "m2,m3,m1", "newest-first sort");

const allSeven = Array.from({ length: 7 }, (_, i) => milestone(`m${i}`, "status_update", i * 1000));
const collapsed = visibleMilestones(allSeven, false);
assert(collapsed.visible.length === VISIBLE_MILESTONE_COLLAPSE, "collapse keeps latest five");
assert(collapsed.hiddenCount === 2, "collapse hidden count");

const expandedAll = visibleMilestones(allSeven, true);
assert(expandedAll.visible.length === 7, "expanded shows all milestones");
assert(expandedAll.hiddenCount === 0, "expanded has no hidden");

const taskA: Task = {
  ...FIXTURE_TASK,
  id: "task-a",
  handle: "alpha",
  milestones: [milestone("a1", "testing", 500)],
};
const taskB: Task = {
  ...FIXTURE_TASK,
  id: "task-b",
  handle: "beta",
  milestones: [milestone("b1", "decision", 900)],
};
const plan = { ...FIXTURE_PLAN, tasks: [taskA, taskB] };
const recent = buildPlanRecentActivity(plan, 5);
assert(recent[0]?.milestone.id === "b1", "plan recent activity is newest first");
assert(recent[0]?.taskLabel === "beta", "plan recent activity uses task handle");
assert(recent.every((item) => plan.tasks.some((task) => task.id === item.taskId)), "recent activity scoped to plan tasks");

const dayTs = new Date(2024, 0, 15, 9, 30).getTime();
const sameDayNow = new Date(2024, 0, 15, 13, 30).getTime();
assert(
  milestoneTimestampLabel(dayTs, sameDayNow) === new Date(dayTs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
  "today entry shortens to time-only",
);
const otherDayNow = new Date(2024, 0, 18, 13, 30).getTime();
assert(milestoneTimestampLabel(dayTs, otherDayNow) === formatTime(dayTs), "non-today entry shows absolute date+time");
assert(milestoneTimestampLabel(dayTs, otherDayNow) !== milestoneTimestampLabel(dayTs, sameDayNow), "same-day and past-day stamps differ");

console.log(`\n✓ taskActivityModel tests passed`);
