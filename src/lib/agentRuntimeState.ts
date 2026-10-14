export interface AgentRuntimeState {
  auth?: "signedIn"|"signedOut"|"unknown";
  loginMethods?: ("browser"|"device"|"apiKey")[];
  loginProviderName?: string;
  loginTerminal?: boolean;
  login?: {status:"pending"|"completed"|"failed"|"canceled";loginId?:string;method?:string;url?:string;userCode?:string;error?:string}|null;
  state: "loading" | "loaded" | "unloaded" | "degraded";
  epoch: number;
  reason?: string | null;
  active_turn_started_at_ms?: number | null;
  last_turn_duration_ms?: number | null;
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
  if (next.state === "loaded") {
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
  if (!current || current.state === "loading" || current.state === "loaded") return current;
  return { ...current, state: "loading" };
}

export function runtimeRetryFailed(current: AgentRuntimeState | null, reason: string): AgentRuntimeState | null {
  if (!current || current.state !== "loading") return current;
  return { ...current, state: "degraded", reason: reason.trim() || current.reason || "Agent runtime unavailable" };
}

const RUNTIME_FAILURE_REASONS: readonly (readonly [RegExp, string])[] = [
  [/\bSizeLimit\b|runtime(?: hook state)? size limit/i, "The conversation history is too large to load."],
  [/^(?:SpawnFailed\b|Runtime could not start:)/, "The agent runtime could not start."],
  [/^(?:ProcessExited\b|Runtime (?:exited|stopped)\b)/, "The agent runtime stopped."],
  [/^(?:PipeEof\b|Runtime connection closed)/, "The connection to the agent runtime closed."],
  [/^(?:PipeFailure\b|Runtime \S+ connection failed:)/, "The connection to the agent runtime failed."],
  [/^(?:InvalidFrame\b|Runtime sent an invalid response:)/, "The agent runtime sent an invalid response."],
  [/^(?:Deadline\b|Runtime did not respond in time)/, "The agent runtime did not respond in time."],
  [/^(?:HookFailed\b|Provider operation failed:)/, "The provider plugin failed while handling a response."],
  [/^(?:Refused\b|Provider refused:)/, "The provider refused the operation."],
  [/^(?:PrewriteRefused\b|Input was not sent:)/, "The input could not be sent to the agent runtime."],
  [/^(?:Superseded\b|Runtime session changed;)/, "The agent session changed and the operation was canceled."],
];

export function runtimeBannerView(state: AgentRuntimeState | null) {
  if (!state || state.state === "loaded") return null;
  return {
    loading: state.state === "loading",
    reason: RUNTIME_FAILURE_REASONS.find(([pattern]) => pattern.test(state.reason ?? ""))?.[1]
      ?? (state.state === "loading" ? "Loading the agent runtime." : "The agent runtime is unavailable."),
    details: state.reason?.trim() ? state.reason : null,
    canRetry: state.state !== "loading",
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
