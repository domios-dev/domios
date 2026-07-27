// Run: npx tsx src/components/Canvas/missionControl/missionControl.test.ts

import type { Pane } from "../../../lib/types";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { TaskStatus, type Plan, type Task, type TaskExecutionAttempt, type LoopSpec } from "../types";
import {
  buildMissionControl,
  MISSION_ZONE_DEFS,
  type MissionControlModel,
  type MissionZoneKey,
} from "./missionControl";

// ── Helpers ────────────────────────────────────────────────────────────────

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const NOW = 1_800_000_000_000;

const LIVE_PANE: Pane = {
  id: "pane-worker",
  pty_id: "pty-1",
  pane_type: "Terminal",
  title: "Puccini",
  cwd: "/repo",
};

const ORCH_PANE: Pane = {
  id: "orch-1",
  pty_id: "pty-0",
  pane_type: "Terminal",
  title: "Orchestrator",
  cwd: "/repo",
};

const PANES: Record<string, Pane> = {
  [LIVE_PANE.id]: LIVE_PANE,
  [ORCH_PANE.id]: ORCH_PANE,
};

function mkTask(over: Partial<Task>): Task {
  return { ...FIXTURE_TASK, milestones: [], ...over };
}

function mkPlan(over: Partial<Plan> & { tasks?: Task[]; loops?: LoopSpec[] }): Plan {
  const { tasks = [], loops = [], ...rest } = over;
  return { ...FIXTURE_PLAN, tasks, loops, ...rest };
}

function zoneStations(model: MissionControlModel, key: MissionZoneKey) {
  return model.zones.find((z) => z.key === key)!.stations;
}

// ── Tests ──────────────────────────────────────────────────────────────────

test("empty plan produces all zones with no stations", () => {
  const model = buildMissionControl(mkPlan({ tasks: [] }), {}, NOW);
  assert(model.planId === "plan-alpha", "planId");
  assert(model.planTitle === "Auth rollout", "planTitle");
  assert(model.activeCount === 0, "activeCount");
  assert(model.deadPaneCount === 0, "deadPaneCount");
  assert(model.presenceByPaneId.size === 0, "no presences");
  const zoneKeys = MISSION_ZONE_DEFS.map((d) => d.key);
  for (const key of zoneKeys) {
    assert(zoneStations(model, key).length === 0, `empty zone: ${key}`);
  }
});

test("cancelled tasks are excluded from all zones", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({ id: "t-cancelled", status: TaskStatus.Cancelled }),
    ],
  });
  const model = buildMissionControl(plan, {}, NOW);
  for (const zone of model.zones) {
    assert(zone.stations.length === 0, `zone ${zone.key} must be empty`);
  }
  assert(model.activeCount === 0, "activeCount");
});

test("pending tasks land in dispatch", () => {
  const plan = mkPlan({
    tasks: [mkTask({ id: "t-pending", status: TaskStatus.Pending })],
  });
  const model = buildMissionControl(plan, {}, NOW);
  assert(zoneStations(model, "dispatch").length === 1, "one in dispatch");
  assert(zoneStations(model, "work_floor").length === 0, "work_floor empty");
  assert(model.activeCount === 0, "pending not active");
});

test("in_progress task with live Tab lands in work_floor with alive presence", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-wip",
        status: TaskStatus.InProgress,
        assigned_pane_id: LIVE_PANE.id,
        status_changed_at: NOW - 60_000,
      }),
    ],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  const stations = zoneStations(model, "work_floor");
  assert(stations.length === 1, "one station in work_floor");
  const st = stations[0];
  assert(st.presence !== null, "presence set");
  assert(st.presence!.alive === true, "Tab alive");
  assert(st.presence!.paneId === LIVE_PANE.id, "paneId");
  assert(st.deadPane === false, "not dead");
  assert(model.activeCount === 1, "activeCount 1");
  assert(model.presenceByPaneId.has(LIVE_PANE.id), "in presenceByPaneId");
});

test("in_progress task with dead Tab is dead_pane flagged", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-ghost",
        status: TaskStatus.InProgress,
        assigned_pane_id: "pane-gone",
        status_changed_at: NOW - 100,
      }),
    ],
  });
  const model = buildMissionControl(plan, {}, NOW);
  const stations = zoneStations(model, "work_floor");
  assert(stations.length === 1, "station present");
  assert(stations[0].deadPane === true, "deadPane flag");
  assert(stations[0].presence === null, "no presence for dead Tab");
  assert(model.deadPaneCount === 1, "deadPaneCount 1");
  assert(model.presenceByPaneId.size === 0, "no live presences");
});

test("in_review non-qa task lands in gate", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-review",
        status: TaskStatus.InReview,
        review_stage: "review",
        assigned_pane_id: LIVE_PANE.id,
        status_changed_at: NOW - 60_000,
      }),
    ],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  assert(zoneStations(model, "gate").length === 1, "gate has one station");
  assert(zoneStations(model, "qa_bay").length === 0, "qa_bay empty");
  assert(model.activeCount === 1, "activeCount 1");
});

test("qa-queued task lands in qa_bay with qaLabel", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-qa",
        status: TaskStatus.InReview,
        review_stage: "qa",
        qa_state: "queued",
        status_changed_at: NOW - 60_000,
      }),
    ],
  });
  const model = buildMissionControl(plan, {}, NOW);
  const stations = zoneStations(model, "qa_bay");
  assert(stations.length === 1, "qa_bay has one");
  assert(stations[0].qaLabel !== null, "qaLabel set");
  assert(stations[0].qaLabel!.includes("QA"), "qaLabel mentions QA");
});

test("done task lands in done_shelf", () => {
  const plan = mkPlan({
    tasks: [mkTask({ id: "t-done", status: TaskStatus.Done })],
  });
  const model = buildMissionControl(plan, {}, NOW);
  assert(zoneStations(model, "done_shelf").length === 1, "done_shelf");
  assert(model.activeCount === 0, "done is not active");
});

test("blocked and paused tasks land in blocked zone", () => {
  const plan = mkPlan({
    tasks: [
      mkTask({ id: "t-blocked", status: TaskStatus.Blocked }),
      mkTask({ id: "t-paused", status: TaskStatus.Paused }),
    ],
  });
  const model = buildMissionControl(plan, {}, NOW);
  assert(zoneStations(model, "blocked").length === 2, "two in blocked");
  assert(model.activeCount === 0, "blocked/paused not active");
});

test("busy active lanes: multiple in_progress with separate Tabs", () => {
  const pane2: Pane = { ...LIVE_PANE, id: "pane-worker-2", title: "Verdi" };
  const plan = mkPlan({
    tasks: [
      mkTask({ id: "t-a", status: TaskStatus.InProgress, assigned_pane_id: LIVE_PANE.id, status_changed_at: NOW - 1000 }),
      mkTask({ id: "t-b", status: TaskStatus.InProgress, assigned_pane_id: pane2.id, status_changed_at: NOW - 2000 }),
    ],
  });
  const allPanes = { ...PANES, [pane2.id]: pane2 };
  const model = buildMissionControl(plan, allPanes, NOW);
  assert(zoneStations(model, "work_floor").length === 2, "two in work_floor");
  assert(model.presenceByPaneId.size === 2, "two live presences");
  assert(model.activeCount === 2, "activeCount 2");
  assert(model.deadPaneCount === 0, "no dead Tabs");
});

test("competition task surfaces competeBadge in work_floor", () => {
  const attempt: TaskExecutionAttempt = {
    pane_id: LIVE_PANE.id,
    pane_title: "Puccini",
    competition_group_id: "grp-1",
    competition_lane: 0,
    closed_at: undefined,
  };
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-compete",
        status: TaskStatus.InProgress,
        assigned_pane_id: LIVE_PANE.id,
        status_changed_at: NOW - 100,
        competition: { enabled: true, attempts: 2 },
        execution_attempts: [attempt],
      }),
    ],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  const st = zoneStations(model, "work_floor")[0];
  assert(st !== undefined, "station exists");
  assert(st.competeBadge !== null, "competeBadge set");
  assert(st.competeBadge!.count === 2, "attempt count 2");
  assert(st.competeBadge!.liveCount === 1, "liveCount 1");
});

test("loop badge surfaces on tasks targeted by a running loop", () => {
  const loop: LoopSpec = {
    id: "loop-1",
    target: "t-loop",
    budget: 5,
    count: 3,
    stop_when: "all green",
    status: "running",
  };
  const plan = mkPlan({
    tasks: [mkTask({ id: "t-loop", status: TaskStatus.InProgress, assigned_pane_id: LIVE_PANE.id, status_changed_at: NOW - 100 })],
    loops: [loop],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  const st = zoneStations(model, "work_floor")[0];
  assert(st !== undefined, "station");
  assert(st.loopBadge !== null, "loopBadge set");
  assert(st.loopBadge!.count === 3, "iteration count");
});

test("stale in_progress task is flagged stale", () => {
  const STALE_MS = 4 * 60 * 60 * 1000 + 1;
  const plan = mkPlan({
    tasks: [
      mkTask({
        id: "t-stale",
        status: TaskStatus.InProgress,
        assigned_pane_id: LIVE_PANE.id,
        status_changed_at: NOW - STALE_MS,
        updated_at: NOW - STALE_MS,
        milestones: [],
      }),
    ],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  const st = zoneStations(model, "work_floor")[0];
  assert(st.stale === true, "stale flag");
});

test("task without assigned Tab has null presence and no deadPane flag", () => {
  const plan = mkPlan({
    tasks: [mkTask({ id: "t-free", status: TaskStatus.InProgress, assigned_pane_id: null, lane: undefined })],
  });
  const model = buildMissionControl(plan, PANES, NOW);
  const st = zoneStations(model, "work_floor")[0];
  assert(st.presence === null, "no presence");
  assert(st.deadPane === false, "not deadPane — no Tab id at all");
});

test("latest milestone note surfaces as latestNote on the station", () => {
  const task = mkTask({
    id: "t-note",
    status: TaskStatus.InProgress,
    assigned_pane_id: LIVE_PANE.id,
    status_changed_at: NOW - 100,
    milestones: [
      { ...FIXTURE_MILESTONE, id: "m1", ts: NOW - 2000, text: "phase one done" },
      { ...FIXTURE_MILESTONE, id: "m2", ts: NOW - 1000, text: "phase two done" },
    ],
  });
  const plan = mkPlan({ tasks: [task] });
  const model = buildMissionControl(plan, PANES, NOW);
  const st = zoneStations(model, "work_floor")[0];
  assert(st.latestNote === "phase two done", "latest note is the newest milestone");
});

test("plan with no title falls back to plan id", () => {
  const plan = mkPlan({ title: "  ", tasks: [] });
  const model = buildMissionControl(plan, {}, NOW);
  assert(model.planTitle === plan.id, "fallback to planId");
});

test("zone order matches MISSION_ZONE_DEFS", () => {
  const model = buildMissionControl(mkPlan({ tasks: [] }), {}, NOW);
  const keys = model.zones.map((z) => z.key);
  const expected = MISSION_ZONE_DEFS.map((d) => d.key);
  assert(JSON.stringify(keys) === JSON.stringify(expected), "zone order preserved");
});

// ── Runner ─────────────────────────────────────────────────────────────────

let passed = 0;
let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
    passed += 1;
  } catch (err) {
    console.error(`  ✗ ${name}`);
    console.error(`    ${(err as Error).message}`);
    failed += 1;
  }
}
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
