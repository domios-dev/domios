import type { Pane } from "../../../lib/types";
import { TaskStatus, type Plan, type Task, type TaskGroup } from "../types";
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
  verify: { queue: FlowChip[]; judging: FlowChip[]; count: number };
  done: { count: number; todayDelta: number };
  carrier: CarrierTransit | null;
  hud: FlowHud;
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

/**
 * Resolve the agent carrying a task across ANY zone (work floor lanes plus
 * gate/QA stations), so in_review chips still dock their avatar. Lanes win
 * (full provider data); model presence + panes covers the rest.
 */
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
    carrier: hot ? agentFor(task.id) : undefined,
  };
}

function orderedGroups(plan: Plan): TaskGroup[] {
  return plan.task_groups ?? [];
}

function groupMembers(plan: Plan, key: string): Task[] {
  return plan.tasks.filter((t) => t.group === key && !isCancelled(t));
}

/**
 * Milestone station derives from task groups: the FIRST group (in plan order)
 * that still has incomplete members is the active milestone. No groups, or all
 * groups complete → inactive ("none active" fallback).
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
    const chips = source
      .slice(0, MAX_STATION_CHIPS)
      .map((t) => chipForTask(agentFor, t, isInFlight(t)));

    return {
      station: {
        active: true,
        key: group.key,
        label: group.label,
        remaining: incomplete.length,
        total: members.length,
        chips,
      },
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
  const groups = orderedGroups(plan);
  const next = groups[activeIndex + 1];
  return next ? next.label : null;
}

function doneToday(plan: Plan, now: number): number {
  return plan.tasks.filter(
    (t) => isDone(t) && typeof t.status_changed_at === "number" && now - t.status_changed_at < DAY_MS,
  ).length;
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

  const verifyQueue = [...snapshot.qaQueue, ...snapshot.gateQueue]
    .slice(0, MAX_STATION_CHIPS)
    .map((t) => ({ taskId: t.id, title: t.title, hot: false }) satisfies FlowChip);
  const judging = snapshot.qaRunning
    ? [{ taskId: snapshot.qaRunning.id, title: snapshot.qaRunning.title, hot: true }]
    : [];

  return {
    planId: snapshot.planId,
    planTitle: snapshot.planTitle,
    progressPct: snapshot.hud.progressPct,
    statusLabel: plan.status !== "active" ? plan.status : null,
    crew,
    crewCount: crew.length,
    frontier: { count: snapshot.dispatchQueue.length, chips: frontierChips },
    milestone,
    verify: {
      queue: verifyQueue,
      judging,
      count: snapshot.qaQueue.length + snapshot.gateQueue.length + judging.length,
    },
    done: { count: snapshot.doneCount, todayDelta: doneToday(plan, now) },
    carrier: buildCarrier(plan, agentFor),
    hud: {
      lanesLive: snapshot.hud.liveLanes,
      qaQueue: snapshot.qaQueue.length,
      loops: snapshot.hud.activeLoops,
      cleanup: snapshot.hud.cleanupDebt,
      next: nextGroupLabel(plan, activeIndex),
    },
  };
}
