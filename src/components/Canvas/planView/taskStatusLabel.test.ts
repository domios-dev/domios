// Run: npx tsx src/components/Canvas/planView/taskStatusLabel.test.ts

import { FIXTURE_MILESTONE, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { canonicalTaskStatusLabel, latestTaskNote } from "./taskStatusLabel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("canonicalTaskStatusLabel ignores status_label", () => {
  assert(
    canonicalTaskStatusLabel({ ...FIXTURE_TASK, status: "in_review", status_label: "In QA · queued #1" }) === "in review",
    "status from task.status only",
  );
});

test("latestTaskNote picks newest milestone and truncates", () => {
  const note = latestTaskNote({
    ...FIXTURE_TASK,
    milestones: [
      { ...FIXTURE_MILESTONE, id: "old", ts: 1, text: "older" },
      { ...FIXTURE_MILESTONE, id: "new", ts: 9, text: "newest note" },
    ],
  });
  assert(note === "newest note", "newest milestone text");
  const long = "x".repeat(130);
  const truncated = latestTaskNote({
    ...FIXTURE_TASK,
    milestones: [{ ...FIXTURE_MILESTONE, text: long }],
  });
  assert(truncated && truncated.length === 120 && truncated.endsWith("…"), "truncates long notes");
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
console.log(`\n${passed}/${tests.length} passed`);
