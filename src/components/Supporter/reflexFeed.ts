import {
  ArrowDownWideNarrow, Bell, CircleCheck, CircleHelp, Clock, CornerUpRight,
  KeyRound, ListChecks, MessageSquare, Moon, Play, RefreshCw, ShieldCheck,
  SkipForward, Square, TriangleAlert, Unplug, Zap, type LucideIcon,
} from "lucide-react";
import { ACTIVITY_ACTIONS, ACTIVITY_KINDS, ACTIVITY_LABELS, ACTIVITY_REASONS, type SupporterActivity } from "./activity";

type Tone = "acted" | "decided" | "quiet" | "skipped" | "fallback";
type Outcome = { icon: LucideIcon; phrase: string; tone: Tone };
const outcome = (icon: LucideIcon, phrase: string, tone: Tone = "decided"): Outcome => ({ icon, phrase, tone });

const kinds: Record<typeof ACTIVITY_KINDS[number], Outcome> = {
  rerank: outcome(ArrowDownWideNarrow, "Ranked memory for recall"),
  tab_state: outcome(CircleHelp, "Checked tab state"),
  recovery: outcome(RefreshCw, "Started recovery", "acted"),
  sentinel: outcome(ListChecks, "Evaluated Blueprint"),
  blueprint: outcome(ListChecks, "Evaluated Blueprint"),
  heartbeat: outcome(ListChecks, "Checked Team progress"),
};
const labels: Record<typeof ACTIVITY_LABELS[number], Outcome> = {
  action_needed: outcome(Zap, "needs action"),
  nothing_changed: outcome(Moon, "has no changes", "quiet"),
  status_only: outcome(ListChecks, "has a status update", "quiet"),
  ranked: outcome(ArrowDownWideNarrow, "Ranked memory for recall"),
  relevance_scored: outcome(ArrowDownWideNarrow, "Scored memory relevance"),
  ready_idle: outcome(CircleCheck, "is ready and idle", "quiet"),
  working_normally: outcome(Play, "is working normally", "quiet"),
  provider_update_prompt: outcome(RefreshCw, "needs a provider update"),
  trust_or_permission_prompt: outcome(ShieldCheck, "needs trust or permission"),
  login_or_auth_required: outcome(KeyRound, "needs authentication"),
  model_unavailable_or_quota: outcome(TriangleAlert, "needs an available model or quota"),
  crashed_to_shell: outcome(TriangleAlert, "exited to the shell"),
  dead_pty: outcome(Unplug, "lost its terminal"),
  error_loop: outcome(TriangleAlert, "is stuck in an error loop"),
  other: outcome(CircleHelp, "could not be classified"),
  still_starting: outcome(Clock, "is still starting", "quiet"),
};
const actions: Record<typeof ACTIVITY_ACTIONS[number], Outcome> = {
  wake: outcome(Zap, "Woke the Orchestrator", "acted"),
  suppress: outcome(Moon, "Let the Orchestrator sleep", "quiet"),
  spawn_ok: outcome(CircleCheck, "Confirmed launch readiness"),
  strong_action: outcome(Zap, "Authorized a response", "decided"),
  rerank: kinds.rerank,
  recovery: kinds.recovery,
  notify_only: outcome(Bell, "Flagged a state for attention"),
  decision: outcome(ListChecks, "Evaluated Blueprint"),
  route_claim: outcome(CornerUpRight, "Claimed a Blueprint route", "decided"),
  notify: outcome(Bell, "Sent a notification", "acted"),
  send: outcome(MessageSquare, "Sent the Blueprint message", "acted"),
  cron: outcome(Clock, "Scheduled the Blueprint task", "acted"),
  stop: outcome(Square, "Stopped the Blueprint", "acted"),
  skip: outcome(SkipForward, "Skipped the action", "skipped"),
};
const reasons: Record<typeof ACTIVITY_REASONS[number], string> = {
  model_answer: "model decision", all_candidates_scored: "all candidates scored",
  launch_ready: "launch ready", startup_deadline: "startup deadline reached",
  startup_escalation: "startup needs attention", worker_idle_without_report: "worker idle without a report",
  busy_without_output: "no new output", tab_exit: "tab exited",
  provider_declared_safe_choice: "provider safe choice", deterministic_directory_trust: "directory trust verified",
  verified_provider_absence: "provider absence verified", confirmed_provider_exit_to_live_shell: "provider exited to shell",
  watchdog_dead_pty_event: "terminal lost", resumed: "session resumed", fresh_launch: "fresh launch",
  budget: "request cap reached", credit_exhausted: "credit exhausted", deadline: "decision timed out",
  unavailable: "model unavailable", no_adapter: "no decision adapter", hard_wake: "action needed",
  same_snapshot: "nothing changed", healthy_snapshot: "Team is healthy", soft_cooldown: "wake cooldown",
  safety_status: "safety needs attention", transition: "state changed", same_bucket: "state unchanged",
  low_confidence: "uncertain decision", other: "unclassified state", budget_exhausted: "request cap reached",
  model_deadline: "decision timed out", model_failed: "model failed", adapter_disconnected: "adapter disconnected",
  invalid_answer: "invalid model answer", route_failed: "route failed",
  provider_safe_step_authorized: "safe provider step authorized", provider_safe_step_stale: "safe provider step expired",
  observation_bounds: "observation unavailable",
};

function lookup<T>(table: Record<string, T>, key: string): T | undefined {
  return Object.prototype.hasOwnProperty.call(table, key) ? table[key] : undefined;
}

export interface ReflexRow extends Outcome {
  key: string;
  kind: string;
  confidence: string;
  subject?: string;
  fallback: boolean;
  count: number;
  time: number;
}

export function reflexOutcome(entry: SupporterActivity): ReflexRow {
  const kind = lookup(kinds, entry.kind);
  const action = lookup(actions, entry.action);
  const label = lookup(labels, entry.label);
  const reason = lookup(reasons, entry.reason);
  const subject = entry.tab_title?.replace(/\s+/g, " ").trim() || undefined;
  let result = action ?? kind ?? outcome(CircleHelp, "Recorded a decision");
  if (entry.kind === "tab_state" && label && ["notify_only", "spawn_ok"].includes(entry.action)) {
    result = { ...label, phrase: `${subject ?? "Tab"} ${label.phrase}` };
  } else if (entry.kind === "rerank") {
    result = label && ["ranked", "relevance_scored"].includes(entry.label) ? label : kinds.rerank;
  } else if (entry.action === "recovery") {
    const completed = ["resumed", "fresh_launch"].includes(entry.reason);
    const safeStep = ["provider_safe_step_authorized", "provider_declared_safe_choice", "deterministic_directory_trust"].includes(entry.reason);
    result = outcome(safeStep ? ShieldCheck : RefreshCw,
      `${safeStep ? "Authorized a safe step for" : completed ? "Recovered" : "Started recovery for"} ${subject ?? "tab"}`,
      safeStep ? "decided" : "acted");
    if (label && entry.label !== "other") result.phrase += ` (${label.phrase})`;
  } else if (subject) {
    result = { ...result, phrase: `${result.phrase} · ${subject}` };
  }
  if (entry.reason === "route_failed") result = outcome(TriangleAlert, "Could not complete the Blueprint route", "skipped");
  if (entry.reason === "provider_safe_step_stale") result = outcome(SkipForward, `Skipped an expired safe step${subject ? ` for ${subject}` : ""}`, "skipped");
  const quietReason = ["model_answer", "all_candidates_scored", "launch_ready"].includes(entry.reason);
  const phrase = `${result.phrase}${reason && !quietReason ? `: ${reason}` : ""}`;
  const fallback = entry.fallback === true;
  return {
    ...result, phrase, subject, fallback, tone: fallback ? "fallback" : result.tone,
    kind: kind ? entry.kind : "unknown",
    confidence: ["strong", "weak", "low", "unknown"].includes(entry.confidence_band) ? entry.confidence_band : "unknown",
    key: JSON.stringify([entry.kind, entry.action, entry.label, subject, fallback]),
    count: 1, time: entry.time,
  };
}

export function groupReflexes(entries: readonly SupporterActivity[]): ReflexRow[] {
  const rows: ReflexRow[] = [];
  for (const entry of entries) {
    const row = reflexOutcome(entry);
    const previous = rows[rows.length - 1];
    if (previous?.key === row.key) {
      const count = previous.count + 1;
      if (Number.isFinite(row.time) && (!Number.isFinite(previous.time) || row.time > previous.time)) {
        Object.assign(previous, row);
      }
      previous.count = count;
    } else rows.push(row);
  }
  return rows;
}
