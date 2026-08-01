// Run: npx tsx src/components/Canvas/planView/taskQa.test.ts

import assert from "node:assert/strict";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { qaChipText, qaQueuePosition } from "./taskQa";

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

function planWith(tasks: Task[]): Plan {
  return { ...FIXTURE_PLAN, tasks };
}

const t = task({
  id: "t-queued",
  status: TaskStatus.InReview,
  review_stage: "qa",
  qa_state: "queued",
  status_label: "blocked: reviewer host unavailable overnight",
});

assert.equal(
  qaChipText(planWith([t]), t),
  "In QA · queued #1 — blocked: reviewer host unavailable overnight",
  "queued QA chip states the queue state and the operator's reason",
);

const noLabel = task({
  id: "t-queued-bare",
  status: TaskStatus.InReview,
  review_stage: "qa",
  qa_state: "queued",
  status_label: null,
});

assert.equal(
  qaChipText(planWith([noLabel]), noLabel),
  "In QA · queued #1",
  "an absent label degrades honestly instead of inventing a reason",
);

const running = task({
  id: "t-running",
  status: TaskStatus.InReview,
  review_stage: "qa",
  qa_state: "running",
  status_label: "irrelevant while running",
});

assert.equal(
  qaChipText(planWith([running]), running),
  "In QA · running",
  "a running QA task is not presented as queued, and its label is not appended",
);

const plainInReview = task({
  id: "t-plain",
  status: TaskStatus.InReview,
  review_stage: "review",
  qa_state: null,
  status_label: null,
});

assert.equal(
  qaChipText(planWith([plainInReview]), plainInReview),
  null,
  "a task outside the QA stage yields no QA chip",
);

const second = task({
  id: "t-second",
  status: TaskStatus.InReview,
  review_stage: "qa",
  qa_state: "queued",
  status_label: null,
});
const plan = planWith([t, second]);
assert.equal(qaQueuePosition(plan, t), 1, "first queued task holds position 1");
assert.equal(qaQueuePosition(plan, second), 2, "second queued task holds position 2");

console.log("taskQa tests passed");
