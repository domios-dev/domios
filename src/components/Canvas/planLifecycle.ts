import type { PlanMode } from "./types";

export type PlanLifecycleKind = "pause" | "resume";
export type PlanPauseRole = "orchestrator" | "planner" | "worker";
export type PlanPauseEffect =
  | "plan_reminders"
  | "heartbeat_nudges"
  | "system_notifications"
  | "team_schedules";

export interface PlanLifecyclePreviewAgent {
  tab_id: string;
  name: string;
  role: PlanPauseRole;
  available: boolean;
}

export interface PlanLifecyclePreviewSchedule {
  job_id: string;
  name: string;
  prior_enabled: boolean;
}

/** Backend-computed preview of what one plan pause or resume operation will change. */
export interface PlanLifecyclePreview {
  kind: PlanLifecycleKind;
  plan_id: string;
  plan_title: string;
  mode: PlanMode;
  applicable: boolean;
  not_applicable_reason: string | null;
  operation_id: string | null;
  restore_mode: PlanMode | null;
  agents: PlanLifecyclePreviewAgent[];
  schedules: PlanLifecyclePreviewSchedule[];
  effects: PlanPauseEffect[];
}

export type PlanLifecyclePreviewLoader = (
  planId: string,
  kind: PlanLifecycleKind,
) => Promise<PlanLifecyclePreview>;

export type PlanLifecycleConfirm = (planId: string, kind: PlanLifecycleKind) => Promise<void>;

export const PLAN_PAUSE_ROLE_LABEL: Record<PlanPauseRole, string> = {
  orchestrator: "Orchestrator",
  planner: "Planner",
  worker: "Worker",
};

export const PLAN_PAUSE_EFFECT_LABEL: Record<PlanPauseEffect, string> = {
  plan_reminders: "Plan reminders",
  heartbeat_nudges: "Heartbeat nudges",
  system_notifications: "Plan notices",
  team_schedules: "Team schedules",
};
