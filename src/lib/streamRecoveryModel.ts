import type { StreamEnd } from "./transportClient";

export function streamRecoveryAction(reason: StreamEnd["reason"], stopped: boolean): "stop" | "wait" {
  if (stopped || reason === "abort") return "stop";
  return "wait";
}
