import type { StreamEnd } from "./transportClient";

export function streamRecoveryAction(reason: StreamEnd["reason"], stopped: boolean): "stop" | "reconnect" | "wait" {
  if (stopped || reason === "abort") return "stop";
  return reason === "eof" ? "reconnect" : "wait";
}
