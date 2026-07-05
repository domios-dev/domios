// Run: npx tsx src/components/Canvas/planView/workingOnRow.test.ts

import type { Pane } from "../../../lib/types";
import { FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Task } from "../types";
import { deriveWorkingOnRow, type WorkingOnChip } from "./workingOnRow";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function task(overrides: Partial<Task>): Task {
  return { ...FIXTURE_TASK, ...overrides };
}

function chip(chips: WorkingOnChip[], kind: WorkingOnChip["kind"]): WorkingOnChip | undefined {
  return chips.find((c) => c.kind === kind);
}

function pane(id: string, title: string): Record<string, Pane> {
  return { [id]: { id, title } as unknown as Pane };
}

test("returns null for non-working statuses", () => {
  assert(deriveWorkingOnRow(task({ status: TaskStatus.Pending }), {}) === null, "pending");
  assert(deriveWorkingOnRow(task({ status: TaskStatus.Done }), {}) === null, "done");
});

test("resolves local pane title, model and branch as info chips", () => {
  const t = task({
    status: TaskStatus.InProgress,
    model: "claude:opus",
    assigned_pane_id: "pane-abcdef12",
    lane: { branch: "lane/x" },
    status_changed_at: 1_000,
  });
  const row = deriveWorkingOnRow(t, pane("pane-abcdef12", "Verdi"), 1_000)!;
  assert(chip(row.chips, "model")?.label === "claude:opus", "model chip");
  const p = chip(row.chips, "pane");
  assert(p?.label === "Verdi" && p.tone === "info", "pane title chip");
  assert(chip(row.chips, "branch")?.label === "lane/x", "branch chip");
});

test("missing branch yields amber warn chip", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_pane_id: "p1", lane: {} });
  const row = deriveWorkingOnRow(t, pane("p1", "P"), 0)!;
  const warn = row.chips.filter((c) => c.kind === "warn");
  assert(warn.some((c) => c.label === "branch missing" && c.tone === "amber"), "branch missing amber");
});

test("no assigned pane yields amber no-pane-recorded chip", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_pane_id: null, lane: {} });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  assert(
    row.chips.some((c) => c.kind === "warn" && c.label === "no pane recorded" && c.tone === "amber"),
    "no pane recorded amber",
  );
});

test("pane recorded but absent locally with no peer is red pane-gone", () => {
  const t = task({ status: TaskStatus.InReview, assigned_pane_id: "ghost", peer: null, lane: { branch: "b" } });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  assert(
    row.chips.some((c) => c.kind === "warn" && c.label === "pane gone" && c.tone === "red"),
    "pane gone red",
  );
});

test("remote pane keeps peer chip and grey short-id, never pane-gone", () => {
  const t = task({
    status: TaskStatus.InProgress,
    assigned_pane_id: "remotepane01",
    peer: "eight",
    lane: { branch: "b" },
  });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  assert(!row.chips.some((c) => c.label === "pane gone"), "no pane gone when peer present");
  assert(chip(row.chips, "peer")?.label === "eight", "peer chip");
  assert(chip(row.chips, "pane")?.tone === "grey", "remote pane grey short id");
});

test("age chip reflects activity age", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_pane_id: "p1", status_changed_at: 60_000 });
  const row = deriveWorkingOnRow(t, pane("p1", "P"), 180_000)!;
  assert(chip(row.chips, "age")?.label === "2m", "2m age");
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
