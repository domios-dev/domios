// Run: npx tsx src/components/Canvas/missionControl/flowModel.test.ts

import type { Tab } from "../../../lib/types";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task, type TaskGroup } from "../types";
import { buildMissionControl } from "./missionControl";
import { modelToSnapshot } from "./modelAdapter";
import { buildFlowRiver } from "./flowModel";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const NOW = 1_800_000_000_000;

function mkTask(id: string, status: TaskStatus, extra: Partial<Task> = {}): Task {
  return { ...FIXTURE_TASK, id, name: id, status, section_ids: [], milestones: [], ...extra };
}

function mkPlan(tasks: Task[], groups?: TaskGroup[]): Plan {
  return { ...FIXTURE_PLAN, tasks, loops: [], task_groups: groups };
}

function river(plan: Plan, tabs: Record<string, Tab> = {}) {
  const model = buildMissionControl(plan, tabs, NOW);
  const snapshot = modelToSnapshot({ plan, model, tabs });
  return buildFlowRiver(plan, model, snapshot, tabs, NOW);
}

const GROUPS: TaskGroup[] = [
  { key: "wave-25", label: "Wave 25", color: null },
  { key: "wave-26", label: "Wave 26", color: null },
  { key: "wave-27", label: "Wave 27", color: null },
];

test("milestone derives from the first incomplete task group with N-of-M", () => {
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.Done, { group: "wave-25" }),
      mkTask("t2", TaskStatus.Done, { group: "wave-25" }),
      mkTask("t3", TaskStatus.InProgress, { group: "wave-26" }),
      mkTask("t4", TaskStatus.Pending, { group: "wave-26" }),
      mkTask("t5", TaskStatus.Done, { group: "wave-26" }),
      mkTask("t6", TaskStatus.Pending, { group: "wave-27" }),
    ],
    GROUPS,
  );
  const m = river(plan).milestone;
  assert(m.active, "milestone active");
  assert(m.label === "Wave 26", `label is Wave 26, got ${m.label}`);
  assert(m.remaining === 2, `remaining 2 (t3,t4), got ${m.remaining}`);
  assert(m.total === 3, `total 3 members, got ${m.total}`);
});

test("milestone skips fully-complete groups", () => {
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.Done, { group: "wave-25" }),
      mkTask("t2", TaskStatus.InProgress, { group: "wave-27" }),
    ],
    GROUPS,
  );
  const m = river(plan).milestone;
  assert(m.label === "Wave 27", `skips complete wave-25, got ${m.label}`);
});

test("next hud points at the group after the active milestone", () => {
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.InProgress, { group: "wave-26" }),
      mkTask("t2", TaskStatus.Pending, { group: "wave-27" }),
    ],
    GROUPS,
  );
  assert(river(plan).hud.next === "Wave 27", "next is Wave 27");
});

test("fallback: no task groups → milestone none active", () => {
  const plan = mkPlan([mkTask("t1", TaskStatus.InProgress), mkTask("t2", TaskStatus.Pending)]);
  const m = river(plan).milestone;
  assert(!m.active, "milestone inactive without groups");
  assert(m.label === "none active", `fallback label, got ${m.label}`);
  assert(m.chips.length === 0, "no chips in fallback");
});

test("fallback: all groups complete → milestone none active", () => {
  const plan = mkPlan([mkTask("t1", TaskStatus.Done, { group: "wave-25" })], GROUPS);
  assert(!river(plan).milestone.active, "all-complete → inactive");
});

test("frontier + verify + done derive from snapshot zones", () => {
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.Pending, { group: "wave-26" }),
      mkTask("t2", TaskStatus.InReview, { group: "wave-26", review_stage: "review" }),
      mkTask("t3", TaskStatus.Done, { group: "wave-26" }),
    ],
    GROUPS,
  );
  const r = river(plan);
  assert(r.frontier.count === 1, `frontier 1 pending, got ${r.frontier.count}`);
  assert(r.verify.count >= 1, `verify has the in_review task, got ${r.verify.count}`);
  assert(r.done.count === 1, `done 1, got ${r.done.count}`);
});

test("done today counts only tasks closed within 24h", () => {
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.Done, { group: "wave-26", status_changed_at: NOW - 1000 }),
      mkTask("t2", TaskStatus.Done, { group: "wave-26", status_changed_at: NOW - 2 * 86_400_000 }),
      mkTask("t3", TaskStatus.InProgress, { group: "wave-26" }),
    ],
    GROUPS,
  );
  assert(river(plan).done.todayDelta === 1, "one done in last 24h");
});

test("carrier surfaces a live in_review task with its agent", () => {
  const tab: Tab = { id: "tab-w", pty_id: "p", tab_type: "Terminal", title: "Verdi", cwd: "/r" };
  const plan = mkPlan(
    [
      mkTask("t1", TaskStatus.InReview, {
        group: "wave-26",
        review_stage: "review",
        assigned_tab_id: "tab-w",
        lane: { tab_id: "tab-w", branch: "lane/x" },
      }),
    ],
    GROUPS,
  );
  const r = river(plan, { "tab-w": tab });
  assert(r.carrier != null, "carrier present for live in_review");
  assert(r.carrier!.label.includes("→ gate"), "carrier label points to gate");
});

test("F1: milestone derives from Task.group when task_groups registry is empty", () => {
  // Real plans often stamp Task.group but leave task_groups empty.
  const plan = mkPlan([
    mkTask("t1", TaskStatus.Done, { group: "Storage-Infra" }),
    mkTask("t2", TaskStatus.InProgress, { group: "Canvas-UX" }),
    mkTask("t3", TaskStatus.Pending, { group: "Canvas-UX" }),
  ]); // no groups registry
  const m = river(plan).milestone;
  assert(m.active, "milestone active from task groups without registry");
  assert(m.label === "Canvas UX", `derived+prettified label, got ${m.label}`);
  assert(m.remaining === 2 && m.total === 2, `N-of-M from group members, got ${m.remaining}/${m.total}`);
});

test("F1: mixed-case Task.group values collapse to one group", () => {
  const plan = mkPlan([
    mkTask("t1", TaskStatus.InProgress, { group: "canvas-ux" }),
    mkTask("t2", TaskStatus.Pending, { group: "Canvas-UX" }),
  ]);
  const m = river(plan).milestone;
  assert(m.total === 2, `case-insensitive group merge, got ${m.total}`);
});

test("F5: open decision gates surface in verify with question + waits-on", () => {
  const tab: Tab = { id: "tab-plan", pty_id: "p", tab_type: "Terminal", title: "Planner", cwd: "/r" };
  const plan: Plan = {
    ...mkPlan([mkTask("t1", TaskStatus.Pending, { group: "wave-26" })], GROUPS),
    gates: [
      { id: "g1", question: "Pick layout variant?", status: "open", asked_at: 1, routed_to_tab_id: "tab-plan" },
      { id: "g2", question: "Answered one", status: "answered", asked_at: 1 },
    ],
  };
  const r = river(plan, { "tab-plan": tab });
  assert(r.verify.gates.length === 1, `only open gates, got ${r.verify.gates.length}`);
  assert(r.verify.gates[0].question === "Pick layout variant?", "gate question");
  assert(r.verify.gates[0].waitsOn === "Planner", `waits on routed Tab, got ${r.verify.gates[0].waitsOn}`);
  assert(r.verify.count >= 1, "gate counts toward verify content");
});

test("F4: all-done plan with no groups is empty (head-only)", () => {
  const plan = mkPlan([mkTask("t1", TaskStatus.Done), mkTask("t2", TaskStatus.Cancelled)]);
  const r = river(plan);
  assert(r.isEmpty === false, "done>0 is not empty; shows Done station");
  assert(!r.milestone.active, "no active milestone");
  const trulyEmpty = river(mkPlan([mkTask("t1", TaskStatus.Cancelled)]));
  assert(trulyEmpty.isEmpty, "all-cancelled plan is empty");
});

test("F1: strides surfaced as last recorded milestone in done", () => {
  const plan: Plan = {
    ...mkPlan([mkTask("t1", TaskStatus.Done, { group: "wave-26" })], GROUPS),
    strides: [
      { id: "s1", from: { tab_id: null, session_id: null, title: "x" }, kind: "wave", text: "Wave 24 shipped", ts: 10, links: [], loop_id: null, iteration: null, canvas: { x: null, y: null, collapsed: false } },
      { id: "s2", from: { tab_id: null, session_id: null, title: "x" }, kind: "wave", text: "Wave 25 shipped", ts: 20, links: [], loop_id: null, iteration: null, canvas: { x: null, y: null, collapsed: false } },
    ],
  };
  assert(river(plan).done.lastStride === "Wave 25 shipped", "latest stride surfaced");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`  ✗ ${name}: ${(e as Error).message}`);
  }
}
if (failed > 0) {
  console.error(`\n${failed} of ${tests.length} flowModel test(s) failed`);
  process.exit(1);
}
console.log(`\n✓ all ${tests.length} flowModel test(s) green`);
