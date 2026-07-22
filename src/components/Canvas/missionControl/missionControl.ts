import type { Pane } from "../../../lib/types";
import { resolvePlanExclusivePaneTasks } from "../paneCurrentTask";
import { presenceColor } from "../presence/identity";
import type { PresenceAgent } from "../presence/identity";
import { isActiveWorkStale } from "../planView/activeWork";
import { competitionForTask } from "../planView/competitionForTask";
import type { CompetitionForTaskResult } from "../planView/competitionForTask";
import { loopForTask } from "../planView/loopForTask";
import type { LoopForTaskResult } from "../planView/loopForTask";
import { qaChipText } from "../planView/taskQa";
import { latestTaskNote } from "../planView/taskStatusLabel";
import { buildTaskPresence } from "../taskPresence";
import { TaskStatus, type Plan, type Task } from "../types";

// ── Zone classification ────────────────────────────────────────────────────

export type MissionZoneKey = "dispatch" | "work_floor" | "gate" | "qa_bay" | "blocked" | "done_shelf";

export interface MissionZoneDef {
  key: MissionZoneKey;
  label: string;
}

export const MISSION_ZONE_DEFS: ReadonlyArray<MissionZoneDef> = [
  { key: "dispatch", label: "Dispatch" },
  { key: "work_floor", label: "Work Floor" },
  { key: "gate", label: "Gate" },
  { key: "qa_bay", label: "QA Bay" },
  { key: "blocked", label: "Blocked" },
  { key: "done_shelf", label: "Done" },
];

// ── Presence ───────────────────────────────────────────────────────────────

export interface WorkerPresence {
  paneId: string;
  label: string;
  role: string | null;
  color: string;
  /** Pane exists in the live panes record — false means the worker is gone. */
  alive: boolean;
  agents: PresenceAgent[];
  branch: string | null;
}

// ── Task station — one task's occupancy in a zone ─────────────────────────

export interface TaskStation {
  task: Task;
  /** Live presence when the executor pane is alive and the task is active. */
  presence: WorkerPresence | null;
  /** True when task has an assigned/lane pane id but the pane is not in panes. */
  deadPane: boolean;
  loopBadge: LoopForTaskResult | null;
  competeBadge: CompetitionForTaskResult | null;
  qaLabel: string | null;
  latestNote: string | null;
  stale: boolean;
}

// ── Zone ───────────────────────────────────────────────────────────────────

export interface MissionZone {
  key: MissionZoneKey;
  label: string;
  stations: TaskStation[];
}

// ── Top-level model ────────────────────────────────────────────────────────

export interface MissionControlModel {
  planId: string;
  planTitle: string;
  zones: ReadonlyArray<MissionZone>;
  /** Live worker presences keyed by pane id — stable map for overlay layers. */
  presenceByPaneId: ReadonlyMap<string, WorkerPresence>;
  /** Count of in_progress + in_review tasks (glanceable health number). */
  activeCount: number;
  /** Count of tasks where the executor pane is gone. */
  deadPaneCount: number;
}

// ── Internals ──────────────────────────────────────────────────────────────

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text || null;
}

function assignedPaneId(task: Task): string | null {
  return clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
}

function taskZone(task: Task): MissionZoneKey {
  switch (task.status) {
    case TaskStatus.Pending: return "dispatch";
    case TaskStatus.InProgress: return "work_floor";
    case TaskStatus.InReview: return task.review_stage === "qa" ? "qa_bay" : "gate";
    case TaskStatus.Done: return "done_shelf";
    case TaskStatus.Blocked:
    case TaskStatus.Paused: return "blocked";
    default: return "dispatch";
  }
}

function buildWorkerPresence(
  task: Task,
  paneId: string,
  panes: Record<string, Pane>,
  agents: PresenceAgent[],
  branch: string | null,
): WorkerPresence {
  const alive = paneId in panes;
  return {
    paneId,
    label: clean(panes[paneId]?.title) ?? paneId.slice(0, 8),
    role: clean(task.taken_by_role),
    color: presenceColor(paneId),
    alive,
    agents,
    branch,
  };
}

function buildStation(
  plan: Plan,
  task: Task,
  panes: Record<string, Pane>,
  exclusivePaneTasks: ReadonlyMap<string, string>,
  now: number,
): TaskStation {
  const rawPresence = buildTaskPresence(plan, task, panes, exclusivePaneTasks);
  const paneId = assignedPaneId(task);
  const deadPane = paneId != null && !(paneId in panes);

  let presence: WorkerPresence | null = null;
  if (rawPresence) {
    presence = buildWorkerPresence(task, rawPresence.paneId, panes, rawPresence.agents, rawPresence.branch);
  } else if (paneId && !deadPane) {
    // Pane exists but was excluded by exclusive-task resolution (not the winner).
    // Still surface the pane so the UI can show the assignment.
    presence = buildWorkerPresence(task, paneId, panes, [], clean(task.lane?.branch));
  }

  return {
    task,
    presence,
    deadPane,
    loopBadge: loopForTask(plan, task.id),
    competeBadge: competitionForTask(task, panes),
    qaLabel: qaChipText(plan, task),
    latestNote: latestTaskNote(task),
    stale: isActiveWorkStale(task, now),
  };
}

// ── Public builder ─────────────────────────────────────────────────────────

export function buildMissionControl(
  plan: Plan,
  panes: Record<string, Pane>,
  now: number = Date.now(),
): MissionControlModel {
  const exclusivePaneTasks = resolvePlanExclusivePaneTasks(plan);

  const zoneStations = new Map<MissionZoneKey, TaskStation[]>(
    MISSION_ZONE_DEFS.map((def) => [def.key, []]),
  );

  const presenceByPaneId = new Map<string, WorkerPresence>();
  let activeCount = 0;
  let deadPaneCount = 0;

  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) continue;

    const zone = taskZone(task);
    const station = buildStation(plan, task, panes, exclusivePaneTasks, now);
    zoneStations.get(zone)!.push(station);

    if (task.status === TaskStatus.InProgress || task.status === TaskStatus.InReview) activeCount += 1;
    if (station.deadPane) deadPaneCount += 1;
    if (station.presence?.alive) presenceByPaneId.set(station.presence.paneId, station.presence);
  }

  const zones: MissionZone[] = MISSION_ZONE_DEFS.map((def) => ({
    key: def.key,
    label: def.label,
    stations: zoneStations.get(def.key)!,
  }));

  return {
    planId: plan.id,
    planTitle: plan.title.trim() || plan.id,
    zones,
    presenceByPaneId,
    activeCount,
    deadPaneCount,
  };
}
