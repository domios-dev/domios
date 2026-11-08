import type { AgentRuntimeState } from "./agentRuntimeState";
import type { ErrorDescription } from "./describeError";
import type { BootStage, Tab, TabStatus } from "./types";

export type TabFailureKind = "ended" | "launch_failed" | "session_unavailable" | "runtime_needs_recovery" | "runtime_stopped" | "agent_error";
export type TabRecovery = "restart" | "start_new_session" | null;

export interface TabFailure {
  kind: TabFailureKind;
  label: string;
  recovery: TabRecovery;
}

export interface TabFailureFacts {
  status?: TabStatus | null;
  bootStage?: BootStage | null;
  runtime?: AgentRuntimeState | null;
  /** A session was bound or the launch reached ready, so a later failure is not a launch failure. */
  launched: boolean;
  /** A chat runtime has no thread to resume before its first session, so only a new session can recover it. */
  chatBacked?: boolean;
}

type TabSessionFacts = Pick<Partial<Tab>, "session_bound" | "agent_session_id" | "backing" | "boot_stage">;

export function tabSessionBound(tab: TabSessionFacts | null | undefined): boolean {
  return tab?.session_bound === true || Boolean(tab?.agent_session_id?.trim())
    || (tab?.backing?.kind === "chat_backend" && Boolean(tab.backing.session_id?.trim()));
}

export function tabLaunched(tab: TabSessionFacts | null | undefined): boolean {
  return tabSessionBound(tab) || tab?.boot_stage === "agent_ready";
}

// One constant per state: activity selectors compare by identity, so a fresh object would rerender forever.
const ENDED: TabFailure = { kind: "ended", label: "Ended", recovery: null };
const LAUNCH_FAILED: TabFailure = { kind: "launch_failed", label: "Launch failed", recovery: "restart" };
const START_UNBOUND: TabFailure = { ...LAUNCH_FAILED, recovery: "start_new_session" };
const SESSION_UNAVAILABLE: TabFailure = { kind: "session_unavailable", label: "Session unavailable", recovery: "start_new_session" };
const RUNTIME_NEEDS_RECOVERY: TabFailure = { kind: "runtime_needs_recovery", label: "Runtime needs recovery", recovery: "restart" };
const RUNTIME_STOPPED: TabFailure = { kind: "runtime_stopped", label: "Runtime stopped", recovery: "restart" };
const AGENT_ERROR: TabFailure = { kind: "agent_error", label: "Agent error", recovery: "restart" };

/** The one mapping from a failed tab's facts to its name and recovery; the failure code decides, never a reason's wording. */
export function tabFailure({ status, bootStage, runtime, launched, chatBacked }: TabFailureFacts): TabFailure {
  if (status === "Dead") return ENDED;
  if (runtime?.state === "degraded" && runtime.failureCode === "session_load_refused") return SESSION_UNAVAILABLE;
  if (runtime?.state === "degraded" && runtime.failureCode === "runtime_start_unbound") return START_UNBOUND;
  if (!launched) return chatBacked ? START_UNBOUND : LAUNCH_FAILED;
  // A failed relaunch of a bound session is still that session's runtime failing, not a launch that never happened.
  if (runtime?.state === "degraded") return RUNTIME_NEEDS_RECOVERY;
  if (runtime?.state === "unloaded") return RUNTIME_STOPPED;
  return bootStage === "spawn_failed" ? LAUNCH_FAILED : AGENT_ERROR;
}

const RUNTIME_CAUSES: Partial<Record<TabFailureKind, string>> = {
  runtime_needs_recovery: "The agent runtime stopped responding. Restart it to continue this conversation.",
  runtime_stopped: "The agent runtime is not running. Restart it to continue this conversation.",
};

/** A live runtime failure reads by its state; launch and session failures keep their own specific description. */
export function tabFailureDescription(failed: TabFailure | undefined, reason: string | null | undefined): ErrorDescription | null {
  const cause = failed && RUNTIME_CAUSES[failed.kind];
  return cause ? { title: failed.label, cause, actions: [], raw: reason?.trim() ?? "" } : null;
}
