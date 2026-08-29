import type { AgentState } from "./AgentStateIndicator";

export type SessionBindingStateKind =
  | "healthy"
  | "deferred_pending"
  | "refused"
  | "unbound"
  | "stale_pending";

export interface SessionBindingFact {
  tab_id: string;
  kind: SessionBindingStateKind;
  current_session_id: string | null;
  requested_session_id: string | null;
  reason: string | null;
}

export interface SessionBindingSurfaceState {
  agentState: AgentState | null;
  rebindSessionId: string | null;
  showRecoveryAction: boolean;
}

export const silentSessionBindingSurface: SessionBindingSurfaceState = {
  agentState: null,
  rebindSessionId: null,
  showRecoveryAction: false,
};

function matchesCurrentSession(fact: SessionBindingFact, ownSessionId: string | null): boolean {
  return fact.current_session_id === ownSessionId;
}

export function sessionBindingSurfaceFromFact(
  ownTabId: string,
  ownSessionId: string | null,
  fact: SessionBindingFact,
): SessionBindingSurfaceState {
  if (fact.tab_id !== ownTabId || !matchesCurrentSession(fact, ownSessionId)) {
    return silentSessionBindingSurface;
  }
  switch (fact.kind) {
    case "deferred_pending":
      return {
        agentState: { state: "quiet", status_line: "Session binding recovering…" },
        rebindSessionId: null,
        showRecoveryAction: false,
      };
    case "refused":
      return {
        agentState: { state: "quiet", status_line: "Session binding refused" },
        rebindSessionId: fact.requested_session_id,
        showRecoveryAction: true,
      };
    case "unbound":
      return {
        agentState: { state: "quiet", status_line: "Session not connected" },
        rebindSessionId: null,
        showRecoveryAction: true,
      };
    case "healthy":
    case "stale_pending":
      return silentSessionBindingSurface;
  }
}
