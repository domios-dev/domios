export type WebReconnectState = "connected" | "pending" | "probing" | "connecting" | "stopped";
export type WebReconnectEvent = "wake" | "deadline" | "lost" | "probe-ok" | "probe-failed" | "connected" | "stop";
export type WebReconnectAction = "none" | "probe" | "connect" | "wait";

export function transitionWebReconnect(
  state: WebReconnectState,
  event: WebReconnectEvent,
): { state: WebReconnectState; action: WebReconnectAction } {
  if (event === "stop") return { state: "stopped", action: "none" };
  if (state === "stopped") return { state, action: "none" };
  if (event === "lost") return { state: "pending", action: "wait" };
  if (event === "connected") return { state: "connected", action: "none" };
  if (state === "pending" && (event === "wake" || event === "deadline")) {
    return { state: "probing", action: "probe" };
  }
  if (state === "probing" && event === "probe-ok") return { state: "connecting", action: "connect" };
  if (state === "probing" && event === "probe-failed") return { state: "pending", action: "wait" };
  return { state, action: "none" };
}

export const RECONNECT_FIRST_MS = 1_000;
export const RECONNECT_CAP_MS = 15_000;
export const RECONNECT_HIDDEN_FIRST_MS = 15_000;
export const RECONNECT_HIDDEN_CAP_MS = 60_000;
export const HEALTHY_STREAM_MS = 30_000;

/** A lost stream gets a bounded attempt schedule: elapsed time only triggers the next
 *  probe, and only a frame or sustained open period counts as recovery. */
export function reconnectDelay(backoffMs: number, random: number, hidden = false): number {
  const floor = hidden ? RECONNECT_HIDDEN_FIRST_MS : RECONNECT_FIRST_MS;
  const cap = hidden ? RECONNECT_HIDDEN_CAP_MS : RECONNECT_CAP_MS;
  const boundedBackoff = Math.min(Math.max(backoffMs, floor), cap);
  return boundedBackoff / 2 + random * (boundedBackoff / 2);
}

export function nextReconnectBackoff(backoffMs: number, hidden = false): number {
  const floor = hidden ? RECONNECT_HIDDEN_FIRST_MS : RECONNECT_FIRST_MS;
  const cap = hidden ? RECONNECT_HIDDEN_CAP_MS : RECONNECT_CAP_MS;
  return Math.min(Math.max(backoffMs, floor) * 2, cap);
}

export function shouldResetReconnectBackoff(sawRealFrame: boolean, openDurationMs: number): boolean {
  return sawRealFrame || openDurationMs >= HEALTHY_STREAM_MS;
}
