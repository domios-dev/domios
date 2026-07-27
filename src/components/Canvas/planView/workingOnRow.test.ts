// Run: npx tsx src/components/Canvas/planView/workingOnRow.test.ts

import type { Tab } from "../../../lib/types";
import { FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Task } from "../types";
import { pickHighestLaneWarning } from "./laneWatchdogLabels";
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

function tab(id: string, title: string): Record<string, Tab> {
  return { [id]: { id, title } as unknown as Tab };
}

test("returns null for non-working statuses", () => {
  assert(deriveWorkingOnRow(task({ status: TaskStatus.Pending }), {}) === null, "pending");
  assert(deriveWorkingOnRow(task({ status: TaskStatus.Done }), {}) === null, "done");
});

test("resolves local Tab title, model and branch as labeled info chips in order", () => {
  const t = task({
    status: TaskStatus.InProgress,
    model: "claude:opus",
    assigned_tab_id: "tab-abcdef12",
    lane: { branch: "lane/x" },
    status_changed_at: 1_000,
  });
  const row = deriveWorkingOnRow(t, tab("tab-abcdef12", "Verdi"), 1_000)!;
  assert(chip(row.chips, "model")?.label === "claude:opus", "model chip");
  assert(chip(row.chips, "model")?.fieldLabel === "Model", "model field label");
  const p = chip(row.chips, "tab");
  assert(p?.label === "Verdi" && p.tone === "info", "tab title chip");
  assert(chip(row.chips, "branch")?.label === "lane/x", "branch chip");
  assert(chip(row.chips, "branch")?.fieldLabel === "Branch", "branch field label");
  const kinds = row.chips.map((c) => c.kind);
  assert(kinds.indexOf("model") < kinds.indexOf("tab"), "model before tab");
  assert(kinds.indexOf("tab") < kinds.indexOf("branch"), "tab before branch");
  assert(kinds.indexOf("branch") < kinds.indexOf("age"), "branch before age");
});

test("missing branch yields amber warn when Tab is recorded", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_tab_id: "p1", lane: {} });
  const row = deriveWorkingOnRow(t, tab("p1", "P"), 0)!;
  const warns = row.chips.filter((c) => c.kind === "warn");
  assert(warns.length === 1, "exactly one warning chip");
  assert(warns[0]?.label === "branch missing" && warns[0].tone === "amber", "branch missing amber");
});

test("no assigned tab yields amber no-tab-recorded chip", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_tab_id: null, lane: {} });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  const warns = row.chips.filter((c) => c.kind === "warn");
  assert(warns.length === 1, "one warning only");
  assert(warns[0]?.label === "no tab recorded" && warns[0].tone === "amber", "no tab recorded amber");
});

test("severity ladder keeps highest warning only", () => {
  const t = task({ status: TaskStatus.InProgress, assigned_tab_id: null, lane: {} });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  assert(row.chips.filter((c) => c.kind === "warn").length === 1, "one warn");
  assert(pickHighestLaneWarning(["branch_missing", "no_tab_recorded"])?.id === "no_tab_recorded", "ladder");
  assert(pickHighestLaneWarning(["branch_missing", "tab_gone"])?.id === "tab_gone", "tab gone wins");
});

test("tab recorded but absent locally with no peer is red tab-gone", () => {
  const t = task({ status: TaskStatus.InReview, assigned_tab_id: "ghost", peer: null, lane: { branch: "b" } });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  const warns = row.chips.filter((c) => c.kind === "warn");
  assert(warns.length === 1, "one warning");
  assert(warns[0]?.label === "tab gone" && warns[0].tone === "red", "tab gone red");
});

test("remote tab keeps peer chip and grey short-id, never tab-gone", () => {
  const t = task({
    status: TaskStatus.InProgress,
    assigned_tab_id: "remotetab01",
    peer: "eight",
    lane: { branch: "b" },
  });
  const row = deriveWorkingOnRow(t, {}, 0)!;
  assert(!row.chips.some((c) => c.label === "tab gone"), "no tab gone when peer present");
  assert(chip(row.chips, "peer")?.label === "eight", "peer chip");
  assert(chip(row.chips, "tab")?.tone === "grey", "remote Tab grey short id");
});

test("age chip is separate from branch", () => {
  const t = task({
    status: TaskStatus.InProgress,
    assigned_tab_id: "p1",
    lane: { branch: "lane/foo" },
    status_changed_at: 60_000,
  });
  const row = deriveWorkingOnRow(t, tab("p1", "P"), 180_000)!;
  assert(chip(row.chips, "age")?.label === "2m", "2m age");
  assert(chip(row.chips, "branch")?.label === "lane/foo", "branch separate");
});

test("middle-truncates long branch slugs keeping tail", () => {
  const long = "lane/task-73ea1b9d-card-metadata";
  const t = task({ status: TaskStatus.InProgress, assigned_tab_id: "p1", lane: { branch: long } });
  const row = deriveWorkingOnRow(t, tab("p1", "P"), 0)!;
  const branch = chip(row.chips, "branch")?.label ?? "";
  assert(branch.includes("…"), "ellipsis present");
  assert(branch.endsWith("card-metadata"), "slug tail preserved");
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
