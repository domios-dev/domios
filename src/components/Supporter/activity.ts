export interface SupporterActivity {
  time: number;
  team_id: string | null;
  kind: string;
  label: string;
  confidence_band: string;
  action: string;
  reason: string;
  budget_reached: boolean | null;
  fallback: boolean;
  tab_title?: string | null;
  tab_id?: string | null;
  asked?: SupporterQuestionAnswer[] | null;
}

export interface SupporterQuestionAnswer {
  name: string;
  question: string;
  answer: string;
}

export const ACTIVITY_LABELS = [
  "action_needed", "nothing_changed", "status_only", "ranked", "relevance_scored", "ready_idle", "working_normally", "provider_update_prompt", "trust_or_permission_prompt", "login_or_auth_required", "model_unavailable_or_quota", "crashed_to_shell", "dead_pty", "error_loop", "other", "still_starting",
] as const;
export const ACTIVITY_ACTIONS = [
  "wake", "suppress", "spawn_ok", "strong_action", "rerank", "recovery", "notify_only",
  "decision", "route_claim", "notify", "send", "cron", "stop", "skip",
] as const;
export const ACTIVITY_REASONS = [
  "model_answer", "all_candidates_scored", "launch_ready", "startup_deadline", "startup_escalation", "worker_idle_without_report", "busy_without_output", "tab_exit", "provider_declared_safe_choice", "deterministic_directory_trust", "verified_provider_absence", "confirmed_provider_exit_to_live_shell", "watchdog_dead_pty_event", "resumed", "fresh_launch", "budget", "credit_exhausted", "deadline", "unavailable", "no_adapter",
  "hard_wake", "same_snapshot", "healthy_snapshot", "soft_cooldown", "safety_status", "transition", "same_bucket", "low_confidence", "other", "budget_exhausted", "model_deadline", "model_failed", "adapter_disconnected", "invalid_answer", "route_failed", "provider_safe_step_authorized", "provider_safe_step_stale", "observation_bounds",
] as const;

export const ACTIVITY_KINDS = ["rerank", "tab_state", "recovery", "sentinel", "blueprint", "heartbeat"] as const;
