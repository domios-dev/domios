import type { Tab } from "../../../lib/types";
import { displayTaskTitle } from "../planView/activeWork";
import { isCompetitiveTask } from "../planView/competitionForTask";
import { TaskStatus, type Plan, type Task } from "../types";
import type { MissionControlModel, MissionZoneKey, TaskStation } from "./missionControl";
import type {
  AgentProvider,
  AgentStationState,
  MissionAgent,
  MissionControlSnapshot,
  MissionLane,
  MissionTask,
  PlanHudMetrics,
  StreamEvent,
} from "./types";

const MAX_STREAM_EVENTS = 4;
const ZONE_EVENT_ANCHOR: Record<MissionZoneKey, { x: number; y: number }> = {
  dispatch: { x: 80, y: 200 },
  work_floor: { x: 420, y: 320 },
  qa_bay: { x: 780, y: 260 },
  gate: { x: 900, y: 340 },
  blocked: { x: 1020, y: 420 },
  done_shelf: { x: 1040, y: 180 },
};

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text || null;
}

function taskHadRecentQaFail(task: Task): boolean {
  const latest = task.milestones.reduce<{ ts: number; kind: string } | null>(
    (best, row) => (!best || row.ts > best.ts ? { ts: row.ts, kind: row.kind } : best),
    null,
  );
  if (!latest) return false;
  const kind = latest.kind.toLowerCase();
  return kind.includes("refused") || kind.includes("qa_fail") || kind.includes("qa-fail");
}

function inferProvider(tab?: Tab): AgentProvider {
  const t = (tab?.agent_type ?? "").toLowerCase();
  if (t.includes("claude")) return "claude";
  if (t.includes("opencode")) return "opencode";
  return "codex";
}

function sectionLabel(task: Task, sectionTitleOf: (sectionId: string) => string | null): string | undefined {
  const id = task.section_ids[0];
  if (!id) return undefined;
  return sectionTitleOf(id) ?? id;
}

function stationState(station: TaskStation): AgentStationState {
  if (taskHadRecentQaFail(station.task)) return "qa-fail";
  if (station.task.status === TaskStatus.InReview) return "reviewing";
  if (!station.presence?.alive) return "waiting";
  if (station.stale) return "idle";
  if (station.task.status === TaskStatus.InProgress) return "working";
  return "idle";
}

function toMissionTask(
  station: TaskStation,
  sectionTitleOf: (sectionId: string) => string | null,
): MissionTask {
  const { task } = station;
  return {
    id: task.id,
    title: displayTaskTitle(task),
    status: task.status,
    laneId: laneKey(station),
    section: sectionLabel(task, sectionTitleOf) ?? undefined,
    workerName: station.presence?.label ?? clean(task.taken_by_role) ?? undefined,
  };
}

function toMissionAgent(
  station: TaskStation,
  tabs: Record<string, Tab>,
  laneId: string,
): MissionAgent | undefined {
  if (!station.presence) return undefined;
  const tab = tabs[station.presence.tabId];
  return {
    id: station.presence.tabId,
    name: station.presence.label,
    provider: inferProvider(tab),
    state: stationState(station),
    laneId,
  };
}

function laneKey(station: TaskStation): string {
  const branch = clean(station.task.lane?.branch);
  if (branch) return `branch:${branch}`;
  const tabId =
    station.presence?.tabId ??
    clean(station.task.assigned_tab_id) ??
    clean(station.task.lane?.tab_id);
  if (tabId) return `tab:${tabId}`;
  return `task:${station.task.id}`;
}

function laneDisplayName(station: TaskStation): string {
  const branch = clean(station.task.lane?.branch);
  if (branch) {
    const slash = branch.lastIndexOf("/");
    return slash >= 0 ? branch.slice(slash + 1) : branch;
  }
  const fallback = station.presence?.label ?? clean(station.task.taken_by_role);
  if (fallback) return fallback;
  const title = displayTaskTitle(station.task);
  return title.length > 28 ? `${title.slice(0, 28).trimEnd()}…` : title;
}

function stationsInZone(model: MissionControlModel, key: MissionZoneKey): TaskStation[] {
  return model.zones.find((z) => z.key === key)?.stations ?? [];
}

function buildLanes(
  workStations: TaskStation[],
  tabs: Record<string, Tab>,
  sectionTitleOf: (sectionId: string) => string | null,
): MissionLane[] {
  const byKey = new Map<string, { name: string; branch: string; stations: TaskStation[] }>();
  for (const station of workStations) {
    const key = laneKey(station);
    let bucket = byKey.get(key);
    if (!bucket) {
      bucket = {
        name: laneDisplayName(station),
        branch: clean(station.task.lane?.branch) ?? clean(station.presence?.branch) ?? "",
        stations: [],
      };
      byKey.set(key, bucket);
    }
    bucket.stations.push(station);
  }

  const lanes: MissionLane[] = [];
  for (const [id, bucket] of byKey) {
    const tasks = bucket.stations.map((s) => toMissionTask(s, sectionTitleOf));
    const agents = bucket.stations
      .map((s) => toMissionAgent(s, tabs, id))
      .filter((a): a is MissionAgent => a != null);
    const primary = bucket.stations[0];
    lanes.push({
      id,
      name: bucket.name,
      branch: bucket.branch,
      agent: agents[0],
      agents: agents.length > 1 ? agents : undefined,
      activeTask: tasks[0],
      activeTasks: tasks.length > 1 || isCompetitiveTask(primary.task) ? tasks : undefined,
    });
  }
  return lanes.sort((a, b) => a.name.localeCompare(b.name));
}

function buildHud(plan: Plan, model: MissionControlModel, lanes: MissionLane[]): PlanHudMetrics {
  const actionable = plan.tasks.filter((t) => t.status !== TaskStatus.Cancelled);
  const done = actionable.filter((t) => t.status === TaskStatus.Done).length;
  const qaDepth = stationsInZone(model, "qa_bay").length;
  const liveLanes = lanes.filter((l) =>
    (l.activeTasks ?? (l.activeTask ? [l.activeTask] : [])).some((t) => t.status === TaskStatus.InProgress),
  ).length;
  const activeLoops = plan.loops.filter((l) => l.status === "running").length;
  const competitiveAttempts = actionable.filter((t) => isCompetitiveTask(t)).length;
  const cleanupDebt = actionable.filter((t) => {
    const status = clean(t.lane?.cleanup_status)?.toLowerCase();
    return status != null && status !== "clean" && status !== "done" && status !== "none";
  }).length;

  return {
    progressPct: actionable.length ? Math.round((done / actionable.length) * 100) : 0,
    liveLanes,
    qaDepth,
    activeLoops,
    competitiveAttempts,
    cleanupDebt,
  };
}

function milestoneKindToEventKind(kind: string): StreamEvent["kind"] {
  const k = kind.toLowerCase();
  if (k.includes("block")) return "blocked";
  if (k.includes("qa")) return "qa_pass";
  if (k.includes("gate") || k.includes("review")) return "gate_open";
  if (k.includes("done") || k.includes("complete")) return "task_done";
  if (k.includes("dispatch") || k.includes("start")) return "dispatch";
  return "task_start";
}

function zoneForTaskStatus(status: Task["status"]): MissionZoneKey {
  switch (status) {
    case TaskStatus.Pending:
      return "dispatch";
    case TaskStatus.InProgress:
      return "work_floor";
    case TaskStatus.InReview:
      return "gate";
    case TaskStatus.Done:
      return "done_shelf";
    default:
      return "blocked";
  }
}

export function extractStreamEvents(plan: Plan, max = MAX_STREAM_EVENTS): StreamEvent[] {
  const rows = plan.tasks.flatMap((task) =>
    task.milestones.map((m) => ({ task, m })),
  );
  rows.sort((a, b) => b.m.ts - a.m.ts);
  return rows.slice(0, max).map(({ task, m }, i) => {
    const zone = zoneForTaskStatus(task.status);
    const anchor = ZONE_EVENT_ANCHOR[zone];
    return {
      id: m.id,
      kind: milestoneKindToEventKind(m.kind),
      label: clean(m.text) ?? displayTaskTitle(task),
      fromX: anchor.x + i * 8,
      fromY: anchor.y + i * 6,
      ts: m.ts,
    };
  });
}

export function missionControlHealth(snapshot: MissionControlSnapshot): {
  dispatchGap: boolean;
  qaBottleneck: boolean;
} {
  const dispatchGap =
    snapshot.dispatchQueue.length > 0 &&
    snapshot.lanes.every((lane) => {
      const tasks = lane.activeTasks ?? (lane.activeTask ? [lane.activeTask] : []);
      return tasks.length === 0;
    });
  return {
    dispatchGap,
    qaBottleneck: snapshot.hud.qaDepth > 3,
  };
}

export interface ModelToSnapshotInput {
  plan: Plan;
  model: MissionControlModel;
  tabs?: Record<string, Tab>;
  sectionTitleOf?: (sectionId: string) => string | null;
}

export function modelToSnapshot({
  plan,
  model,
  tabs = {},
  sectionTitleOf = () => null,
}: ModelToSnapshotInput): MissionControlSnapshot {
  const titleOf = (sectionId: string) => sectionTitleOf(sectionId);
  const mapStations = (key: MissionZoneKey) =>
    stationsInZone(model, key).map((s) => toMissionTask(s, titleOf));

  const qaStations = stationsInZone(model, "qa_bay");
  const qaRunningStation = qaStations.find((s) => s.task.qa_state === "running") ?? qaStations[0];
  const qaQueueStations = qaStations.filter((s) => s !== qaRunningStation);

  const blockedStations = stationsInZone(model, "blocked");
  const paused = mapStations("blocked").filter((_, i) => blockedStations[i]?.task.status === TaskStatus.Paused);
  const blocked = mapStations("blocked").filter((_, i) => blockedStations[i]?.task.status === TaskStatus.Blocked);

  const doneTasks = mapStations("done_shelf");
  const lanes = buildLanes(stationsInZone(model, "work_floor"), tabs, titleOf);
  const goal = clean(plan.goal?.vector) ?? clean(plan.goal?.vision) ?? "";

  return {
    planId: model.planId,
    planTitle: model.planTitle,
    goal,
    hud: buildHud(plan, model, lanes),
    dispatchQueue: mapStations("dispatch"),
    lanes,
    qaQueue: qaQueueStations.map((s) => toMissionTask(s, titleOf)),
    qaRunning: qaRunningStation ? toMissionTask(qaRunningStation, titleOf) : undefined,
    gateQueue: mapStations("gate"),
    doneCount: doneTasks.length,
    doneRecent: doneTasks.slice(-3).reverse(),
    paused,
    blocked,
    events: extractStreamEvents(plan),
  };
}
