
export type PlanLifecycleKind = import("../../../gen/PlanLifecycleKind").PlanLifecycleKind;
export type PlanPauseRole = import("../../../gen/PlanPauseRole").PlanPauseRole;
export type PlanPauseEffect = import("../../../gen/PlanPauseEffect").PlanPauseEffect;

export type PlanLifecyclePreviewAgent = import("../../../gen/PlanLifecyclePreviewAgent").PlanLifecyclePreviewAgent;

export type PlanLifecyclePreviewSchedule = import("../../../gen/PlanLifecyclePreviewSchedule").PlanLifecyclePreviewSchedule;

/** Backend-computed preview of what one plan pause or resume operation will change. */
export type PlanLifecyclePreview = import("../../../gen/PlanLifecyclePreview").PlanLifecyclePreview;

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
