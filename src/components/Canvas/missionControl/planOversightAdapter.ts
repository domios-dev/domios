import type { Plan } from "../types";
import type { PlanControlLoop } from "../planControlLoop";
import type { PlanOversight } from "./planSettingsModel";

export function planOversight(plan: Pick<Plan, "mode" | "status" | "pause_operations">, loop: PlanControlLoop | undefined, unavailableReason: string): PlanOversight {
  const pause = plan.mode === "paused" ? [...(plan.pause_operations ?? [])].reverse().find(operation => operation.resume == null) : undefined;
  return {
    mode: loop?.mode ?? "curator",
    heartbeat_interval_ms: loop?.heartbeat_interval_secs == null ? null : loop.heartbeat_interval_secs * 1000,
    next_at: loop?.next_at ?? null,
    running_round: loop?.running?.number ?? null,
    last_round_at: loop?.last_round?.ended_at ?? null,
    fallback_reason: loop?.fallback_reason?.replace(/_/g, " ") ?? null,
    paused: plan.mode === "paused",
    paused_at: pause?.paused_at ?? null,
    pause_reason: pause?.reason ?? null,
    suspended_reason: plan.mode === "complete" || plan.status !== "active" ? "complete" : loop ? loop.suspended_reason : unavailableReason,
  };
}
