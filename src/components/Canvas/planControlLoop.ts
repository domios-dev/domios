type PlanHydrated<T, K extends keyof T, N extends keyof T = never> = Pick<{ [P in keyof T]: T[P] | (P extends N ? null : never) }, K> & Partial<Omit<{ [P in keyof T]: T[P] | (P extends N ? null : never) }, K>>;

export type ControlLoopMode = import("../../../gen/ControlLoopMode").ControlLoopMode;

export type CuratorRound = PlanHydrated<Pick<import("../../../gen/CuratorRound").CuratorRound, Extract<keyof import("../../../gen/CuratorRound").CuratorRound, "number" | "started_at" | "ended_at" | "outcome" | "summary" | "next_delay_secs" | "reason" | "learnings" | "save_id">>, "number" | "started_at" | "ended_at" | "outcome" | "summary" | "next_delay_secs" | "reason" | "learnings" | "save_id", "ended_at" | "save_id">;

export type PlanControlLoop = Omit<PlanHydrated<Pick<import("../../../gen/ControlLoop").ControlLoop, Extract<keyof import("../../../gen/ControlLoop").ControlLoop, "manual_run_requested" | "running" | "mode" | "heartbeat_interval_secs" | "next_at" | "last_round" | "fallback_reason" | "suspended_reason" | "counts_7d">>, "mode" | "heartbeat_interval_secs" | "next_at" | "last_round" | "fallback_reason" | "suspended_reason" | "counts_7d", "running" | "heartbeat_interval_secs" | "next_at" | "last_round" | "fallback_reason" | "suspended_reason">, "running" | "mode" | "last_round"> & {
  running?: (Pick<import("../../../gen/CuratorRun").CuratorRun, "number"> & Partial<Omit<import("../../../gen/CuratorRun").CuratorRun, "number">>) | null;
  mode: ControlLoopMode;
  last_round: CuratorRound | null;
};

export type ControlLoopUpdate = Omit<PlanHydrated<Pick<import("../../../gen/ControlLoopUpdate").ControlLoopUpdate, Extract<keyof import("../../../gen/ControlLoopUpdate").ControlLoopUpdate, "mode" | "heartbeat_interval_secs">>, "mode", "heartbeat_interval_secs">, "mode"> & {
  mode: ControlLoopMode;
};

