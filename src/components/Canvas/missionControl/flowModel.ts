import type { Pane } from "../../../lib/types";
import { TaskStatus, type DecisionGate, type Plan, type Task } from "../types";
import { displayTaskTitle } from "../planView/activeWork";
import type { MissionControlModel, TaskStation } from "./missionControl";
import type { AgentProvider, AgentStationState, MissionAgent, MissionControlSnapshot } from "./types";

// ── Flow-river model — the FLOW canvas presentation of one plan ──────────────

export interface FlowChip {
  taskId: string;
  title: string;
  hot: boolean;
  carrier?: MissionAgent;
}

/** The glowing center station — the active task group (milestone-gate). */
export interface MilestoneStation {
  active: boolean;
  key: string | null;
  label: string;
  remaining: number;
  total: number;
  chips: FlowChip[];
}

/** An open decision gate whose answer is required before blocked tasks can proceed. */
export interface GateView {
  id: string;
  question: string;
  waitsOn: string;
  escalated: boolean;
}

export interface VerifyStation {
  inReview: FlowChip[];
  gates: GateView[];
  count: number;
}

export interface CarrierTransit {
  agent: MissionAgent;
  label: string;
}

export interface FlowHud {
  lanesLive: number;
  qaQueue: number;
  loops: number;
  cleanup: number;
  next: string | null;
}

export interface FlowRiverModel {
  planId: string;
  planTitle: string;
  progressPct: number;
  statusLabel: string | null;
  crew: MissionAgent[];
  crewCount: number;
  frontier: { count: number; chips: FlowChip[] };
  milestone: MilestoneStation;
  verify: VerifyStation;
  done: { count: number; todayDelta: number; lastStride: string | null };
  carrier: CarrierTransit | null;
  hud: FlowHud;
  /** No actionable work, no gates, no done — render a head-only compact block. */
  isEmpty: boolean;
}

const DAY_MS = 86_400_000;
const MAX_STATION_CHIPS = 3;

function isCancelled(task: Task): boolean {
  return task.status === TaskStatus.Cancelled;
}

function isDone(task: Task): boolean {
  return task.status === TaskStatus.Done;
}

function isInFlight(task: Task): boolean {
  return task.status === TaskStatus.InProgress || task.status === TaskStatus.InReview;
}

function inferProvider(pane?: Pane): AgentProvider {
  const t = (pane?.agent_type ?? "").toLowerCase();
  if (t.includes("claude")) return "claude";
  if (t.includes("opencode")) return "opencode";
  return "codex";
}

function stationState(station: TaskStation): AgentStationState {
  if (station.task.status === TaskStatus.InReview) return "reviewing";
  if (!station.presence?.alive) return "waiting";
  if (station.stale) return "idle";
  if (station.task.status === TaskStatus.InProgress) return "working";
  return "idle";
}

interface AgentResolver {
  (taskId: string): MissionAgent | undefined;
}

function makeAgentResolver(
  snapshot: MissionControlSnapshot,
  model: MissionControlModel,
  panes: Record<string, Pane>,
): AgentResolver {
  const laneAgent = new Map<string, MissionAgent>();
  for (const lane of snapshot.lanes) {
    const tasks = lane.activeTasks ?? (lane.activeTask ? [lane.activeTask] : []);
    const agent = lane.agents?.[0] ?? lane.agent;
    if (!agent) continue;
    for (const t of tasks) laneAgent.set(t.id, agent);
  }

  const stationByTask = new Map<string, TaskStation>();
  for (const zone of model.zones) {
    for (const station of zone.stations) stationByTask.set(station.task.id, station);
  }

  return (taskId) => {
    const fromLane = laneAgent.get(taskId);
    if (fromLane) return fromLane;
    const station = stationByTask.get(taskId);
    const presence = station?.presence;
    if (!station || !presence) return undefined;
    return {
      id: presence.paneId,
      name: presence.label,
      provider: inferProvider(panes[presence.paneId]),
      state: stationState(station),
    };
  };
}

function chipForTask(agentFor: AgentResolver, task: Task, hot: boolean): FlowChip {
  return {
    taskId: task.id,
    title: displayTaskTitle(task),
    hot,
    carrier: agentFor(task.id),
  };
}

// ── Task-group derivation (with fallback) ────────────────────────────────────

interface DerivedGroup {
  key: string; // normalized
  label: string;
}

function normGroup(raw?: string | null): string | null {
  const t = raw?.trim();
  return t ? t.toLowerCase() : null;
}

function prettyLabel(raw: string): string {
  const t = raw.trim().replace(/[-_]+/g, " ");
  return t.replace(/\b\w/g, (c) => c.toUpperCase());
}

/**
 * Ordered milestone groups. Prefer the plan's task_groups registry; when it is
 * empty (common on real plans that only stamp Task.group), derive the ordered
 * distinct groups straight from the tasks — so the milestone station is never a
 * ghost when real group work exists.
 */
function orderedGroups(plan: Plan): DerivedGroup[] {
  const registry = plan.task_groups ?? [];
  if (registry.length > 0) {
    return registry
      .map((g) => ({ key: normGroup(g.key) ?? "", label: g.label?.trim() || prettyLabel(g.key) }))
      .filter((g) => g.key !== "");
  }
  const seen = new Map<string, DerivedGroup>();
  for (const task of plan.tasks) {
    if (isCancelled(task)) continue;
    const key = normGroup(task.group);
    if (!key || seen.has(key)) continue;
    seen.set(key, { key, label: prettyLabel(task.group!.trim()) });
  }
  return [...seen.values()];
}

function groupMembers(plan: Plan, key: string): Task[] {
  return plan.tasks.filter((t) => normGroup(t.group) === key && !isCancelled(t));
}

/**
 * Milestone station: the FIRST group (in order) with incomplete members is the
 * active milestone. No groups / all complete → inactive (compact "none active").
 */
function buildMilestone(plan: Plan, agentFor: AgentResolver): { station: MilestoneStation; activeIndex: number } {
  const groups = orderedGroups(plan);
  for (let i = 0; i < groups.length; i += 1) {
    const group = groups[i];
    const members = groupMembers(plan, group.key);
    if (members.length === 0) continue;
    const incomplete = members.filter((t) => !isDone(t));
    if (incomplete.length === 0) continue;

    const inFlight = incomplete.filter(isInFlight);
    const source = inFlight.length > 0 ? inFlight : incomplete;
    const chips = source.slice(0, MAX_STATION_CHIPS).map((t) => chipForTask(agentFor, t, isInFlight(t)));

    return {
      station: { active: true, key: group.key, label: group.label, remaining: incomplete.length, total: members.length, chips },
      activeIndex: i,
    };
  }
  return {
    station: { active: false, key: null, label: "none active", remaining: 0, total: 0, chips: [] },
    activeIndex: -1,
  };
}

function nextGroupLabel(plan: Plan, activeIndex: number): string | null {
  if (activeIndex < 0) return null;
  const next = orderedGroups(plan)[activeIndex + 1];
  return next ? next.label : null;
}

function doneToday(plan: Plan, now: number): number {
  return plan.tasks.filter(
    (t) => isDone(t) && typeof t.status_changed_at === "number" && now - t.status_changed_at < DAY_MS,
  ).length;
}

/** Latest recorded stride (plan milestone) — proves recorded progress is surfaced. */
function lastStride(plan: Plan): string | null {
  const strides = plan.strides ?? [];
  let best: { ts: number; text: string } | null = null;
  for (const s of strides) {
    const text = s.text?.trim();
    if (!text) continue;
    if (!best || s.ts > best.ts) best = { ts: s.ts, text };
  }
  return best ? best.text : null;
}

function paneLabel(paneId: string | null | undefined, panes: Record<string, Pane>): string {
  const id = paneId?.trim();
  if (!id) return "planner";
  return panes[id]?.title?.trim() || id.slice(0, 8);
}

/** Open decision gates → question + who it's waiting on. */
function buildGates(plan: Plan, panes: Record<string, Pane>): GateView[] {
  return (plan.gates ?? [])
    .filter((g: DecisionGate) => g.status === "open" || g.status === "escalated")
    .map((g) => ({
      id: g.id,
      question: g.question.trim(),
      waitsOn: paneLabel(g.routed_to_pane_id, panes),
      escalated: g.status === "escalated",
    }));
}

function buildVerify(plan: Plan, snapshot: MissionControlSnapshot, panes: Record<string, Pane>, agentFor: AgentResolver): VerifyStation {
  const inReview = plan.tasks
    .filter((t) => t.status === TaskStatus.InReview)
    .slice(0, MAX_STATION_CHIPS)
    .map((t) => chipForTask(agentFor, t, true));
  const gates = buildGates(plan, panes);
  const qaCount = snapshot.qaQueue.length + (snapshot.qaRunning ? 1 : 0);
  return {
    inReview,
    gates,
    count: inReview.length + gates.length + qaCount,
  };
}

/** The in-flight "agent carrying a chip to gate" moment — a live in_review task. */
function buildCarrier(plan: Plan, agentFor: AgentResolver): CarrierTransit | null {
  for (const task of plan.tasks) {
    if (task.status !== TaskStatus.InReview) continue;
    const agent = agentFor(task.id);
    if (agent) return { agent, label: `${displayTaskTitle(task)} → gate` };
  }
  return null;
}

function buildCrew(snapshot: MissionControlSnapshot): MissionAgent[] {
  const byId = new Map<string, MissionAgent>();
  for (const lane of snapshot.lanes) {
    for (const agent of lane.agents ?? (lane.agent ? [lane.agent] : [])) {
      if (!byId.has(agent.id)) byId.set(agent.id, agent);
    }
  }
  return [...byId.values()];
}

export function buildFlowRiver(
  plan: Plan,
  model: MissionControlModel,
  snapshot: MissionControlSnapshot,
  panes: Record<string, Pane> = {},
  now: number = Date.now(),
): FlowRiverModel {
  const agentFor = makeAgentResolver(snapshot, model, panes);
  const { station: milestone, activeIndex } = buildMilestone(plan, agentFor);
  const crew = buildCrew(snapshot);

  const frontierChips = snapshot.dispatchQueue
    .slice(0, MAX_STATION_CHIPS)
    .map((t) => ({ taskId: t.id, title: t.title, hot: false }) satisfies FlowChip);

  const verify = buildVerify(plan, snapshot, panes, agentFor);
  const carrier = buildCarrier(plan, agentFor);
  const done = { count: snapshot.doneCount, todayDelta: doneToday(plan, now), lastStride: lastStride(plan) };

  const isEmpty =
    snapshot.dispatchQueue.length === 0 &&
    !milestone.active &&
    verify.count === 0 &&
    done.count === 0 &&
    !carrier;

  return {
    planId: snapshot.planId,
    planTitle: snapshot.planTitle,
    progressPct: snapshot.hud.progressPct,
    statusLabel: plan.status !== "active" ? plan.status : null,
    crew,
    crewCount: crew.length,
    frontier: { count: snapshot.dispatchQueue.length, chips: frontierChips },
    milestone,
    verify,
    done,
    carrier,
    hud: {
      lanesLive: snapshot.hud.liveLanes,
      qaQueue: snapshot.qaQueue.length,
      loops: snapshot.hud.activeLoops,
      cleanup: snapshot.hud.cleanupDebt,
      next: nextGroupLabel(plan, activeIndex),
    },
    isEmpty,
  };
}
