export const RECONNECT_GRACE_MS = 10_000;
export const RECONNECTED_NOTICE_MS = 2_000;

export type BrandConnectionState =
  | { kind: "connected" }
  | { kind: "waiting"; disconnectedAt: number }
  | { kind: "disconnected"; disconnectedAt: number }
  | { kind: "reconnected"; connectedAt: number };

export type BrandConnectionEvent = "disconnect" | "grace-expired" | "connect" | "notice-expired";

export type BrandStateModel =
  | { kind: "hidden" }
  | { kind: "loading"; label: string; detail?: string; animate: boolean }
  | { kind: "error"; title: string; detail: string }
  | { kind: "reconnecting" }
  | { kind: "reconnected" };

export function transitionBrandConnection(
  state: BrandConnectionState,
  event: BrandConnectionEvent,
  now: number,
): BrandConnectionState {
  if (event === "disconnect") {
    return state.kind === "waiting" || state.kind === "disconnected"
      ? state
      : { kind: "waiting", disconnectedAt: now };
  }
  if (event === "grace-expired") {
    return state.kind === "waiting" && now >= state.disconnectedAt + RECONNECT_GRACE_MS
      ? { kind: "disconnected", disconnectedAt: state.disconnectedAt }
      : state;
  }
  if (event === "connect") {
    if (state.kind === "disconnected") return { kind: "reconnected", connectedAt: now };
    if (state.kind === "waiting") {
      return now >= state.disconnectedAt + RECONNECT_GRACE_MS
        ? { kind: "reconnected", connectedAt: now }
        : { kind: "connected" };
    }
    return state;
  }
  return state.kind === "reconnected" && now >= state.connectedAt + RECONNECTED_NOTICE_MS
    ? { kind: "connected" }
    : state;
}

export function brandConnectionModel(state: BrandConnectionState, now: number): BrandStateModel {
  if (state.kind === "disconnected") return { kind: "reconnecting" };
  if (state.kind === "waiting") {
    return now >= state.disconnectedAt + RECONNECT_GRACE_MS ? { kind: "reconnecting" } : { kind: "hidden" };
  }
  if (state.kind === "reconnected") {
    return now < state.connectedAt + RECONNECTED_NOTICE_MS ? { kind: "reconnected" } : { kind: "hidden" };
  }
  return { kind: "hidden" };
}

export function brandStateModel(input: {
  loading?: boolean;
  label?: string;
  detail?: string;
  animate?: boolean;
  error?: { title: string; detail: string };
  connection?: BrandConnectionState;
  now?: number;
}): BrandStateModel {
  if (input.error) return { kind: "error", ...input.error };
  if (input.loading) {
    return {
      kind: "loading",
      label: input.label ?? "Loading",
      ...(input.detail ? { detail: input.detail } : {}),
      animate: input.animate ?? true,
    };
  }
  if (input.connection && input.now !== undefined) return brandConnectionModel(input.connection, input.now);
  return { kind: "hidden" };
}
