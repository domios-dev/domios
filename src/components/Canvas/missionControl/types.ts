import type { TaskStatus } from "../types";

export type AgentProvider = "claude" | "codex" | "opencode";
export type AgentStationState = "working" | "idle" | "waiting" | "reviewing" | "qa-fail";

export interface MissionAgent {
  id: string;
  name: string;
  provider: AgentProvider;
  state: AgentStationState;
  laneId?: string;
}

export interface MissionTask {
  id: string;
  title: string;
  status: TaskStatus;
  laneId?: string;
  section?: string;
  priority?: number;
  workerName?: string;
}

export interface MissionLane {
  id: string;
  name: string;
  branch: string;
  agent?: MissionAgent;
  /** Competition / multi-worker bays surface multiple avatars. */
  agents?: MissionAgent[];
  activeTask?: MissionTask;
  activeTasks?: MissionTask[];
}

export interface PlanHudMetrics {
  progressPct: number;
  liveLanes: number;
  qaDepth: number;
  activeLoops: number;
  competitiveAttempts: number;
  cleanupDebt: number;
}

export interface StreamEvent {
  id: string;
  kind: "task_start" | "task_done" | "qa_pass" | "gate_open" | "blocked" | "dispatch";
  label: string;
  fromX: number;
  fromY: number;
  ts: number;
}

export interface MissionControlSnapshot {
  planId: string;
  planTitle: string;
  goal: string;
  hud: PlanHudMetrics;
  dispatchQueue: MissionTask[];
  lanes: MissionLane[];
  qaQueue: MissionTask[];
  qaRunning?: MissionTask;
  gateQueue: MissionTask[];
  doneCount: number;
  doneRecent: MissionTask[];
  paused: MissionTask[];
  blocked: MissionTask[];
  events: StreamEvent[];
}

export type MissionControlVariant = "hangar" | "control-room" | "assembly-line";
export type MissionControlLod = "detail" | "overview";

export const MISSION_CONTROL_VARIANTS: MissionControlVariant[] = [
  "assembly-line",
  "hangar",
  "control-room",
];

export const VARIANT_LABELS: Record<MissionControlVariant, string> = {
  "assembly-line": "A · Assembly line",
  hangar: "B · Hangar floor",
  "control-room": "C · Control room",
};
