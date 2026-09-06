// Run: npx tsx src/components/Sidebar/TeamUnit.status.test.ts

import assert from "node:assert/strict";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import { TaskStatus, type Plan } from "../Canvas/types";
import { deriveCardStatusCounts, deriveTeamTaskBuckets } from "./TeamUnit";

const plan: Plan = {
  ...FIXTURE_PLAN,
  tasks: [
    { ...FIXTURE_TASK, id: "pending", status: TaskStatus.Pending },
    { ...FIXTURE_TASK, id: "paused", status: TaskStatus.Paused },
    { ...FIXTURE_TASK, id: "done", status: TaskStatus.Done },
  ],
};

const buckets = deriveTeamTaskBuckets(plan, {});

// Donut buckets stay a true partition of the plan's tasks: paused is never
// folded here, or the ring would double-count tasks across two arcs.
assert.equal(buckets.find((bucket) => bucket.key === "todo")?.count, 1);
assert.equal(buckets.find((bucket) => bucket.key === "paused")?.count, 1);
assert.equal(buckets.find((bucket) => bucket.key === "done")?.count, 1);

// D12: the card aggregate (PlanStatusBreakdown's counts) folds paused into
// todo — 1 pending + 2 paused reads 3 todo — while the detailed breakdown
const d12Plan: Plan = {
  ...FIXTURE_PLAN,
  tasks: [
    { ...FIXTURE_TASK, id: "pending-1", status: TaskStatus.Pending },
    { ...FIXTURE_TASK, id: "paused-1", status: TaskStatus.Paused },
    { ...FIXTURE_TASK, id: "paused-2", status: TaskStatus.Paused },
  ],
};
const d12Buckets = deriveTeamTaskBuckets(d12Plan, {});
const cardCounts = deriveCardStatusCounts(d12Buckets);
assert.equal(cardCounts.todo, 3, "1 pending + 2 paused renders 3 todo on the card aggregate");
assert.equal(cardCounts.paused, 2, "the detailed breakdown still reports paused=2 separately");

console.log("TeamUnit status tests passed");
