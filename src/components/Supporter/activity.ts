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
  asked?: SupporterQuestionAnswer[] | null;
}

export interface SupporterQuestionAnswer {
  name: string;
  question: string;
  answer: string;
}

const labels = [
  "action_needed", "nothing_changed", "status_only", "ranked", "relevance_scored", "ready_idle", "working_normally", "provider_update_prompt", "trust_or_permission_prompt", "login_or_auth_required", "model_unavailable_or_quota", "crashed_to_shell", "dead_pty", "error_loop", "other", "still_starting",
];
const actions = [
  "wake", "suppress", "spawn_ok", "strong_action", "rerank", "recovery", "notify_only",
  "decision", "route_claim", "notify", "send", "cron", "stop", "skip",
];
const reasons = [
  "model_answer", "all_candidates_scored", "launch_ready", "startup_deadline", "startup_escalation", "worker_idle_without_report", "busy_without_output", "tab_exit", "provider_declared_safe_choice", "deterministic_directory_trust", "verified_provider_absence", "confirmed_provider_exit_to_live_shell", "watchdog_dead_pty_event", "resumed", "fresh_launch", "budget", "credit_exhausted", "deadline", "unavailable", "no_adapter",
  "hard_wake", "same_snapshot", "healthy_snapshot", "soft_cooldown", "safety_status", "transition", "same_bucket", "low_confidence", "other", "budget_exhausted", "model_deadline", "model_failed", "adapter_disconnected", "invalid_answer", "route_failed",
];

function safe(value: string, allowed: string[]) {
  return allowed.includes(value) ? value.replaceAll("_", " ") : "redacted";
}

export function activityText(entry: SupporterActivity) {
  return {
    kind: safe(entry.kind, ["rerank", "tab_state", "recovery", "sentinel", "heartbeat"]).replace("rerank", "re-rank").replace("tab state", "tab-state"),
    label: safe(entry.label, labels),
    confidence: safe(entry.confidence_band, ["strong", "weak", "low", "unknown"]),
    action: safe(entry.action, actions),
    reason: safe(entry.reason, reasons),
  };
}

const KIND_TITLES: Record<string, string> = {
  "re-rank": "Memory re-rank",
  "tab-state": "Tab state",
  recovery: "Recovery",
  sentinel: "Blueprint",
  heartbeat: "Team heartbeat",
};

export function activitySummary(text: ReturnType<typeof activityText>, asked?: SupporterQuestionAnswer[] | null) {
  if (asked?.length) {
    const first = asked[0];
    const more = asked.length > 1 ? ` (+${asked.length - 1} questions)` : "";
    return `${first.question} → ${first.answer}${more}`;
  }
  const title = KIND_TITLES[text.kind] ?? text.kind;
  return text.label === "redacted" ? title : `${title}: ${text.label}`;
}
