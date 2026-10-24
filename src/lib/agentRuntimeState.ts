import { describeError, isPendingRuntimeDeadline } from "./describeError";
export interface AgentRuntimeState {
  auth?: "signedIn"|"signedOut"|"unknown";
  loginMethods?: ("browser"|"device"|"apiKey")[];
  loginProviderName?: string;
  loginTerminal?: boolean;
  login?: {status:"pending"|"completed"|"failed"|"canceled";loginId?:string;method?:string;url?:string;userCode?:string;error?:string}|null;
  state: "loading" | "loginPending" | "loaded" | "unloaded" | "degraded";
  epoch: number;
  reason?: string | null;
  active_turn_started_at_ms?: number | null;
  last_turn_duration_ms?: number | null;
  history?: RuntimeHistoryState;
  historyPages?: number;
  historyError?: string;
}

export type RuntimeHistoryState = "loading" | "complete" | "failed";

export function runtimeResponseDeadlinePending(
  state: AgentRuntimeState | null | undefined,
  bootFailureReason?: string | null,
): boolean {
  if (state?.state === "loaded" && state.history !== "failed") return false;
  return isPendingRuntimeDeadline(bootFailureReason)
    || ((state?.state === "degraded" || state?.state === "loading") && isPendingRuntimeDeadline(state.reason))
    || (state?.state === "loaded" && state.history === "failed" && isPendingRuntimeDeadline(state.historyError));
}

/** True when a history replay just finished, so the attached transcript must re-read its snapshot. */
export function runtimeHistorySettled(
  previous: RuntimeHistoryState | undefined,
  next: RuntimeHistoryState | undefined,
): boolean {
  return previous === "loading" && (next === "complete" || next === "failed");
}

export interface AgentRuntimeStateEvent {
  questions?: import("./agentRuntimeQuestions").AgentRuntimeQuestion[];
  tab_id: string;
  session_binding_nonce: string | null;
  state: AgentRuntimeState;
}

export interface RuntimeAuthority {
  tabId: string;
  nonce: string | null;
}

export function runtimeStateFromSnapshot(
  current: AgentRuntimeState | null,
  next: AgentRuntimeState | null | undefined,
): AgentRuntimeState | null {
  if (!next) return null;
  if (current && next.epoch < current.epoch) return current;
  if (next.state === "loaded" || next.state === "loginPending") {
    const loaded = { ...next };
    delete loaded.reason;
    return loaded;
  }
  if (next.state === "loading") return { ...next, reason: next.reason ?? current?.reason };
  return { ...next, reason: next.reason?.trim() || "Agent runtime unavailable" };
}

export function runtimeStateFromEvent(
  current: AgentRuntimeState | null,
  authority: RuntimeAuthority,
  event: AgentRuntimeStateEvent,
): AgentRuntimeState | null {
  if (event.tab_id !== authority.tabId || event.session_binding_nonce !== authority.nonce) return current;
  return runtimeStateFromSnapshot(current, event.state);
}

export function beginRuntimeRetry(current: AgentRuntimeState | null): AgentRuntimeState | null {
  if (current?.state === "loaded" && current.history === "failed") return { ...current, history:"loading", historyError:undefined };
  if (!current || current.state === "loading" || current.state === "loaded") return current;
  return { ...current, state: "loading" };
}

export function runtimeRetryFailed(current: AgentRuntimeState | null, reason: string): AgentRuntimeState | null {
  if (current?.state === "loaded" && current.history === "loading") return { ...current, history:"failed", historyError:reason };
  if (!current || current.state !== "loading") return current;
  return { ...current, state: "degraded", reason: reason.trim() || current.reason || "Agent runtime unavailable" };
}

export function runtimeStatusPresentation(state: AgentRuntimeState | null, startupOwned = false) {
  if (state?.state === "loginPending") return { severity: "action_needed" as const,
    label: "Sign-in required", reason: "Sign in to continue.", details: null, canRetry: true };
  if (runtimeResponseDeadlinePending(state)) {
    if (startupOwned) return null;
    return { severity: "transient" as const, label: "Agent runtime is taking longer than usual",
      reason: "The current runtime response is still pending.",
      details: state?.reason?.trim() || state?.historyError?.trim() || null, canRetry: false };
  }
  if (state?.state === "loaded" && state.history === "failed") {
    const details = state.historyError?.trim() || `History page ${(state.historyPages ?? 0)+1} could not be loaded`;
    return { severity:"action_needed" as const, label:"Chat history incomplete",
      reason:describeError(details,{operation:"loading older messages"}).title, details, canRetry:true };
  }
  if (!state || state.state === "loaded") return null;
  if (startupOwned && state.state === "loading") return null;
  const loading = state.state === "loading";
  return {
    severity: loading ? "transient" as const : "action_needed" as const,
    label: loading ? "Recovering agent runtime" : "Agent runtime unavailable",
    reason: loading ? "Loading the agent runtime."
      : describeError(state.reason, { operation: "connecting to the agent runtime" }).title,
    details: state.reason?.trim() ? state.reason : null,
    canRetry: !loading,
  };
}

export interface AgentRuntimeView {
  state: AgentRuntimeState | null;
  questions: import("./agentRuntimeQuestions").AgentRuntimeQuestion[];
}

export function runtimeViewFromEvent(
  current: AgentRuntimeView, authority: RuntimeAuthority, event: AgentRuntimeStateEvent,
): AgentRuntimeView {
  const state = runtimeStateFromEvent(current.state, authority, event);
  if (state === current.state) return current;
  return { ...current, state, questions: event.questions ?? current.questions };
}
