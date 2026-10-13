export interface AgentRuntimeState {
  state: "loading" | "loaded" | "unloaded" | "degraded";
  epoch: number;
  reason?: string | null;
}

export interface AgentRuntimeStateEvent {
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
  if (next.state === "loaded") return { state: "loaded", epoch: next.epoch };
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
  if (!current || current.state === "loading" || current.state === "loaded") return current;
  return { ...current, state: "loading" };
}

export function runtimeRetryFailed(current: AgentRuntimeState | null, reason: string): AgentRuntimeState | null {
  if (!current || current.state !== "loading") return current;
  return { ...current, state: "degraded", reason: reason.trim() || current.reason || "Agent runtime unavailable" };
}

export function runtimeBannerView(state: AgentRuntimeState | null) {
  if (!state || state.state === "loaded") return null;
  return {
    loading: state.state === "loading",
    reason: state.reason ?? null,
    canRetry: state.state !== "loading",
  };
}
