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
