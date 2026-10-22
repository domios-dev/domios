export type ControlLoopMode = "curator" | "heartbeat";

export interface CuratorRound {
  number: number;
  started_at: number;
  ended_at: number | null;
  outcome: string;
  summary: string;
  next_delay_secs: number;
  reason: string;
  learnings: number;
  save_id: string | null;
}

export interface PlanControlLoop {
  manual_run_requested?: boolean;
  running?: { number: number } | null;
  mode: ControlLoopMode;
  heartbeat_interval_secs: number | null;
  next_at: number | null;
  last_round: CuratorRound | null;
  fallback_reason: string | null;
  suspended_reason: string | null;
  counts_7d: Record<"rounds" | "quiet" | "nudged" | "learnings" | "saves" | "fallbacks", number>;
}

export interface ControlLoopUpdate {
  mode: ControlLoopMode;
  heartbeat_interval_secs?: number | null;
}

