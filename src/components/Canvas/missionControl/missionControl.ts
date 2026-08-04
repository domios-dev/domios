import type { Tab } from "../../../lib/types";
import { resolvePlanExclusiveTabTasks } from "../tabCurrentTask";
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
  tabId: string;
  label: string;
  role: string | null;
  color: string;
  /** Tab exists in the live tabs record — false means the worker is gone. */
  alive: boolean;
  agents: PresenceAgent[];
  branch: string | null;
}

// ── Task station — one task's occupancy in a zone ─────────────────────────

export interface TaskStation {
  task: Task;
  /** Live presence when the executor tab is alive and the task is active. */
  presence: WorkerPresence | null;
  /** True when task has an assigned/lane tab id but the tab is not in tabs. */
  deadTab: boolean;
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
  /** Live worker presences keyed by tab id — stable map for overlay layers. */
  presenceByTabId: ReadonlyMap<string, WorkerPresence>;
  /** Count of in_progress + in_review tasks (glanceable health number). */
  activeCount: number;
  /** Count of tasks where the executor tab is gone. */
  deadTabCount: number;
}

// ── Internals ──────────────────────────────────────────────────────────────

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text || null;
}

function assignedTabId(task: Task): string | null {
  return clean(task.assigned_tab_id) ?? clean(task.lane?.tab_id);
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
  tabId: string,
  tabs: Record<string, Tab>,
  agents: PresenceAgent[],
  branch: string | null,
): WorkerPresence {
  const alive = tabId in tabs;
  return {
    tabId,
    label: clean(tabs[tabId]?.title) ?? tabId.slice(0, 8),
    role: clean(task.taken_by_role),
    color: presenceColor(tabId),
    alive,
    agents,
    branch,
  };
}

function buildStation(
  plan: Plan,
  task: Task,
  tabs: Record<string, Tab>,
  exclusiveTabTasks: ReadonlyMap<string, string>,
  now: number,
): TaskStation {
  const rawPresence = buildTaskPresence(plan, task, tabs, exclusiveTabTasks);
  const tabId = assignedTabId(task);
  const deadTab = tabId != null && !(tabId in tabs);

  let presence: WorkerPresence | null = null;
  if (rawPresence) {
    presence = buildWorkerPresence(task, rawPresence.tabId, tabs, rawPresence.agents, rawPresence.branch);
  } else if (tabId && !deadTab) {
    // Tab exists but was excluded by exclusive-task resolution (not the winner).
    // Still surface the tab so the UI can show the assignment.
    presence = buildWorkerPresence(task, tabId, tabs, [], clean(task.lane?.branch));
  }

  return {
    task,
    presence,
    deadTab,
    loopBadge: loopForTask(plan, task.id),
    competeBadge: competitionForTask(task, tabs),
    qaLabel: qaChipText(plan, task),
    latestNote: latestTaskNote(task),
    stale: isActiveWorkStale(task, now),
  };
}

// ── Public builder ─────────────────────────────────────────────────────────

export function buildMissionControl(
  plan: Plan,
  tabs: Record<string, Tab>,
  now: number = Date.now(),
): MissionControlModel {
  const exclusiveTabTasks = resolvePlanExclusiveTabTasks(plan);

  const zoneStations = new Map<MissionZoneKey, TaskStation[]>(
    MISSION_ZONE_DEFS.map((def) => [def.key, []]),
  );

  const presenceByTabId = new Map<string, WorkerPresence>();
  let activeCount = 0;
  let deadTabCount = 0;

  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) continue;

    const zone = taskZone(task);
    const station = buildStation(plan, task, tabs, exclusiveTabTasks, now);
    zoneStations.get(zone)!.push(station);

    if (task.status === TaskStatus.InProgress || task.status === TaskStatus.InReview) activeCount += 1;
    if (station.deadTab) deadTabCount += 1;
    if (station.presence?.alive) presenceByTabId.set(station.presence.tabId, station.presence);
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
    presenceByTabId,
    activeCount,
    deadTabCount,
  };
}
