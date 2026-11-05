import type { PlanView } from "../planProjection";
import { layoutPlanIntra } from "../layout/intraPlan";
import { layoutInputRevision } from "../layout/layoutInputRevision";
import { buildTaskPresenceFingerprint } from "../canvasView/presenceFingerprint";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
// Run: npx tsx src/components/Canvas/missionControl/modelAdapter.test.ts

import type { Tab } from "../../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task } from "../types";
import { buildMissionControl } from "./missionControl";
import { missionControlHealth, modelToSnapshot } from "./modelAdapter";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const NOW = 1_800_000_000_000;
const TAB: Tab = {
  id: "tab-a",
  pty_id: "pty-a",
  tab_type: "Terminal",
  title: "Worker A",
  cwd: "/repo",
  agent_type: "claude",
};

function mkTask(over: Partial<Task>): Task {
  return { ...FIXTURE_TASK, milestones: [], ...over };
}

function mkPlan(tasks: Task[]): Plan {
  return { ...FIXTURE_PLAN, tasks, loops: [] };
}

test("metadata-only projection reaches canvas layout and mission control without document fields", () => {
  const plan: PlanView = {
    id: "pl_metadata", title: "Metadata", status: "active",
    owner: { tab_id: "owner", session_id: null, title: "Owner" }, project_path: "/repo", created_at: 1, updated_at: 2,
    canvas: { viewport: null }, sections: [{ id: "sec_metadata", title: "Metadata", subsections: [] }],
    tasks: [{
      id: "t_metadata", name: "Projected task", type: "task", status: TaskStatus.Pending,
      model: null, status_label: null, parent_task_id: null, assigned_tab_id: null,
      peer: null, section_ids: ["sec_metadata"], canvas: { x: null, y: null, collapsed: false }, milestone_count: 4,
    }],
  };
  const layout = layoutPlanIntra(plan, DEFAULT_CANVAS_SETTINGS);
  assert(layout.nodes.some((node) => node.id === "t_metadata"), "projected task laid out");
  assert(layout.nodes.every((node) => Number.isFinite(node.box.height)), "finite node geometry");
  assert(layoutInputRevision([plan]).includes("t_metadata"), "layout revision includes projected task");
  assert(buildTaskPresenceFingerprint([plan], {}).includes("t_metadata"), "presence fingerprint includes assignment");
  const model = buildMissionControl(plan, {}, NOW);
  const snapshot = modelToSnapshot({ plan, model });
  assert(snapshot.dispatchQueue[0]?.id === "t_metadata", "metadata task remains dispatchable");
  assert(snapshot.events.length === 0, "absent history supplies no events");
  assert(plan.tasks[0].milestones === undefined && plan.elements === undefined, "projection remains unfilled");
});

test("modelToSnapshot maps dispatch queue from pending tasks", () => {
  const plan = mkPlan([mkTask({ id: "t1", status: TaskStatus.Pending })]);
  const model = buildMissionControl(plan, {}, NOW);
  const snap = modelToSnapshot({ plan, model });
  assert(snap.dispatchQueue.length === 1, "dispatch count");
  assert(snap.dispatchQueue[0]!.id === "t1", "task id");
});

test("modelToSnapshot groups work_floor stations into lanes by branch", () => {
  const plan = mkPlan([
    mkTask({
      id: "t-wip",
      status: TaskStatus.InProgress,
      assigned_tab_id: TAB.id,
      lane: { branch: "lane/feature-a" },
      status_changed_at: NOW - 1000,
    }),
  ]);
  const model = buildMissionControl(plan, { [TAB.id]: TAB }, NOW);
  const snap = modelToSnapshot({ plan, model, tabs: { [TAB.id]: TAB } });
  assert(snap.lanes.length === 1, "one lane");
  assert(snap.lanes[0]!.branch === "lane/feature-a", "branch");
  assert(snap.lanes[0]!.activeTask?.id === "t-wip", "active task");
  assert(snap.lanes[0]!.agent?.provider === "claude", "provider");
});

test("missionControlHealth flags dispatch gap when dispatch full and floor empty", () => {
  const plan = mkPlan([
    mkTask({ id: "t-p", status: TaskStatus.Pending }),
    mkTask({ id: "t-p2", status: TaskStatus.Pending }),
  ]);
  const model = buildMissionControl(plan, {}, NOW);
  const snap = modelToSnapshot({ plan, model });
  const health = missionControlHealth(snap);
  assert(health.dispatchGap === true, "dispatch gap");
});

test("missionControlHealth flags QA bottleneck when depth > 3", () => {
  const tasks = Array.from({ length: 4 }, (_, i) =>
    mkTask({
      id: `qa-${i}`,
      status: TaskStatus.InReview,
      review_stage: "qa",
      qa_state: "queued",
    }),
  );
  const plan = mkPlan(tasks);
  const model = buildMissionControl(plan, {}, NOW);
  const snap = modelToSnapshot({ plan, model });
  assert(missionControlHealth(snap).qaBottleneck === true, "bottleneck");
});

test("modelToSnapshot caps stream events", () => {
  const milestones = Array.from({ length: 10 }, (_, i) => ({
    ...FIXTURE_MILESTONE,
    id: `m-${i}`,
    text: `event ${i}`,
    ts: NOW - i * 1000,
  }));
  const plan = mkPlan([mkTask({ id: "t1", status: TaskStatus.InProgress, milestones })]);
  const model = buildMissionControl(plan, {}, NOW);
  const snap = modelToSnapshot({ plan, model });
  assert(snap.events.length <= 4, `events capped: ${snap.events.length}`);
});

test("modelToSnapshot splits blocked vs paused in side rails", () => {
  const plan = mkPlan([
    mkTask({ id: "t-b", status: TaskStatus.Blocked }),
    mkTask({ id: "t-p", status: TaskStatus.Paused }),
  ]);
  const model = buildMissionControl(plan, {}, NOW);
  const snap = modelToSnapshot({ plan, model });
  assert(snap.blocked.length === 1 && snap.blocked[0]!.id === "t-b", "blocked");
  assert(snap.paused.length === 1 && snap.paused[0]!.id === "t-p", "paused");
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
console.log(`\n✓ ${passed}/${tests.length} modelAdapter tests passed`);
