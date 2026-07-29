// Run: npx tsx src/components/Sidebar/TeamUnit.status.test.ts

import assert from "node:assert/strict";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import { TaskStatus, type Plan } from "../Canvas/types";
import { deriveTeamTaskBuckets } from "./TeamUnit";

const plan: Plan = {
  ...FIXTURE_PLAN,
  tasks: [
    { ...FIXTURE_TASK, id: "pending", status: TaskStatus.Pending },
    { ...FIXTURE_TASK, id: "paused", status: TaskStatus.Paused },
    { ...FIXTURE_TASK, id: "done", status: TaskStatus.Done },
  ],
};

const buckets = deriveTeamTaskBuckets(plan, {});

assert.equal(buckets.find((bucket) => bucket.key === "todo")?.count, 1);
assert.equal(buckets.find((bucket) => bucket.key === "paused")?.count, 1);
assert.equal(buckets.find((bucket) => bucket.key === "done")?.count, 1);
console.log("TeamUnit status tests passed");
