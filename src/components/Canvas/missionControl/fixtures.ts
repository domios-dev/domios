import { TaskStatus } from "../types";
import type { MissionControlSnapshot, MissionLane, MissionTask } from "./types";

function task(
  id: string,
  title: string,
  status: MissionTask["status"],
  extra: Partial<MissionTask> = {},
): MissionTask {
  return { id, title, status, ...extra };
}

const LANES: MissionLane[] = [
  {
    id: "lane_canvas",
    name: "Canvas",
    branch: "lane/canvas-mission-control-concepts",
    agent: { id: "ag_puccini", name: "Puccini", provider: "codex", state: "working", laneId: "lane_canvas" },
    activeTask: task("task_4d59", "Mission control concept design", TaskStatus.InProgress, {
      section: "Canvas UX",
      workerName: "Puccini",
    }),
  },
  {
    id: "lane_rust",
    name: "Backend",
    branch: "lane/plan-canvas-api",
    agent: { id: "ag_verdi", name: "Verdi", provider: "claude", state: "working", laneId: "lane_rust" },
    activeTask: task("task_8f2a", "Canvas element sync API", TaskStatus.InProgress, {
      section: "API",
      workerName: "Verdi",
    }),
  },
  {
    id: "lane_pw",
    name: "Playwright",
    branch: "lane/canvas-pw-golden",
    agent: { id: "ag_mozart", name: "Mozart", provider: "opencode", state: "idle", laneId: "lane_pw" },
  },
  {
    id: "lane_docs",
    name: "Docs",
    branch: "lane/orchestration-docs",
    agent: { id: "ag_bach", name: "Bach", provider: "claude", state: "waiting", laneId: "lane_docs" },
    activeTask: task("task_doc1", "Motion spec + artifact README", TaskStatus.InReview, {
      section: "Docs",
      workerName: "Bach",
    }),
  },
];

export const MISSION_CONTROL_FIXTURE: MissionControlSnapshot = {
  planId: "pl_dd93d764",
  planTitle: "Canvas mission control",
  goal: "Turn Canvas into the plan operations room — dispatch, lanes, QA, gate, done.",
  hud: {
    progressPct: 62,
    liveLanes: 3,
    qaDepth: 4,
    activeLoops: 2,
    competitiveAttempts: 1,
    cleanupDebt: 3,
  },
  dispatchQueue: [
    task("task_d1", "Wire live lane bay updates", TaskStatus.Pending, { section: "Canvas", priority: 1 }),
    task("task_d2", "Gate desk orchestrator actions", TaskStatus.Pending, { section: "Orchestration", priority: 2 }),
    task("task_d3", "Done shelf compression LOD", TaskStatus.Pending, { section: "Canvas", priority: 3 }),
    task("task_d4", "Event particle HUD feed", TaskStatus.Pending, { section: "Motion", priority: 4 }),
  ],
  lanes: LANES,
  qaQueue: [
    task("task_q1", "Prototype gallery route", TaskStatus.Pending, { section: "Frontend" }),
    task("task_q2", "Fixture unit tests", TaskStatus.Pending, { section: "Testing" }),
    task("task_q3", "Static HTML artifact", TaskStatus.Pending, { section: "Deliverables" }),
  ],
  qaRunning: task("task_q0", "Mission control layout variants", TaskStatus.InProgress, {
    section: "Design",
    workerName: "QA bot",
  }),
  gateQueue: [
    task("task_g1", "User gate: pick layout variant", TaskStatus.InReview, { section: "Gate", workerName: "Orchestrator" }),
    task("task_g2", "Motion spec review", TaskStatus.InReview, { section: "Motion" }),
  ],
  doneCount: 18,
  doneRecent: [
    task("task_done1", "Plan model TaskStatus mirror", TaskStatus.Done),
    task("task_done2", "Canvas worlds v3 spec", TaskStatus.Done),
    task("task_done3", "Section zones prototype", TaskStatus.Done),
  ],
  paused: [
    task("task_p1", "Live canvas wiring", TaskStatus.Paused, { section: "Deferred" }),
    task("task_p2", "Mesh presence avatars", TaskStatus.Paused, { section: "Future" }),
  ],
  blocked: [
    task("task_b1", "Production screenshot pipeline", TaskStatus.Blocked, { section: "CI" }),
    task("task_b2", "Windows WebView2 particle perf", TaskStatus.Blocked, { section: "Platform" }),
  ],
  events: [
    { id: "ev1", kind: "dispatch", label: "Dispatched task_d1", fromX: 120, fromY: 180, ts: Date.now() - 4000 },
    { id: "ev2", kind: "task_start", label: "Puccini → lane_canvas", fromX: 420, fromY: 320, ts: Date.now() - 2800 },
    { id: "ev3", kind: "qa_pass", label: "QA slot opened", fromX: 780, fromY: 260, ts: Date.now() - 1200 },
    { id: "ev4", kind: "gate_open", label: "Gate: user review", fromX: 900, fromY: 340, ts: Date.now() - 600 },
  ],
};

export function dispatchFrontierCount(snapshot: MissionControlSnapshot): number {
  return snapshot.dispatchQueue.length;
}

export function activeLaneCount(snapshot: MissionControlSnapshot): number {
  return snapshot.lanes.filter((l) => l.activeTask?.status === TaskStatus.InProgress).length;
}

export function totalBlockedPaused(snapshot: MissionControlSnapshot): number {
  return snapshot.blocked.length + snapshot.paused.length;
}
