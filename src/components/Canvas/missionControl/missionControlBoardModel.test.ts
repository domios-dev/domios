// Run: npx tsx src/components/Canvas/missionControl/missionControlBoardModel.test.ts

import {
  SMALL_SAMPLE_MAX,
  doneCaption,
  openCaption,
  progressRatio,
  progressReading,
} from "./missionControlBoardModel";
import type { PlanTaskCounts } from "./missionControlState";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function eq<T>(actual: T, expected: T, message: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${message}: expected ${e}, got ${a}`);
}

function counts(partial: Partial<PlanTaskCounts>): PlanTaskCounts {
  const base = { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, ...partial };
  const total = base.pending + base.in_progress + base.in_review + base.blocked + base.paused + base.done + base.cancelled;
  return { ...base, total };
}

const RELEASE = counts({ pending: 82, in_progress: 6, in_review: 2, blocked: 5, done: 464, cancelled: 28 });

test("segments run done first and cancelled last, skipping empty statuses", () => {
  const reading = progressReading(RELEASE);
  eq(reading.segments.map((s) => s.key), ["done", "in_review", "in_progress", "blocked", "pending", "cancelled"], "order");
  eq(reading.segments.map((s) => s.count), [464, 2, 6, 5, 82, 28], "counts");
  eq(reading.segments.find((s) => s.key === "in_progress")?.label, "in progress", "label");
  eq(reading.total, 587, "total");
});

test("done leaves cancelled out of the whole and reads as a percent on a large plan", () => {
  const { done } = progressReading(RELEASE);
  eq(done, { part: 464, whole: 559, percent: 83 }, "done ratio");
  eq(doneCaption(done), "464 of 559", "caption");
});

test("open counts every task that can still move, blocked and paused included", () => {
  const reading = progressReading(counts({ pending: 82, in_progress: 6, in_review: 2, blocked: 5, paused: 3, done: 9 }));
  eq(reading.open, 98, "open");
  eq(reading.blocked, 5, "blocked");
  eq(openCaption(reading.open), "of 98 open", "caption");
  eq(openCaption(0), "nothing open", "nothing open");
});

test("a small plan keeps a plain fraction and says what is left", () => {
  const { done } = progressReading(counts({ done: 1, in_progress: 1, pending: 5 }));
  eq(done, { part: 1, whole: 7, percent: null }, "fraction");
  eq(doneCaption(done), "6 to go", "to go");
  eq(doneCaption(progressRatio(0, 3)), "just started", "nothing done");
  eq(doneCaption(progressRatio(2, 2)), "all done", "everything done");
});

test("the percent starts exactly at the small-sample bound", () => {
  assert(progressRatio(1, SMALL_SAMPLE_MAX - 1).percent === null, "below the bound");
  eq(progressRatio(1, SMALL_SAMPLE_MAX).percent, 10, "at the bound");
});

test("an empty or fully cancelled plan has no whole to measure", () => {
  const empty = progressReading(counts({}));
  eq(empty.segments, [], "no segments");
  eq(empty.done.whole, 0, "empty whole");
  eq(doneCaption(empty.done), "no tasks yet", "empty caption");

  const cancelled = progressReading(counts({ cancelled: 4 }));
  eq(cancelled.segments.map((s) => s.key), ["cancelled"], "cancelled segment");
  eq(cancelled.done, { part: 0, whole: 0, percent: null }, "cancelled whole");
});

test("a finished plan reads one hundred percent", () => {
  const { done } = progressReading(counts({ done: 203, cancelled: 9 }));
  eq(done.percent, 100, "percent");
  eq(doneCaption(done), "203 of 203", "caption");
});

let passed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    console.error(`  ✗ ${name}`);
    console.error(e);
    process.exit(1);
  }
}
console.log(`\n✓ ${passed}/${tests.length} missionControlBoardModel tests passed`);
