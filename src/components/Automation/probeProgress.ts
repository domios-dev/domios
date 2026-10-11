import { automationState } from "./model";
import type { AutomationNode } from "./types";

export function probeStateLabel(node: AutomationNode): string {
  const state = automationState(node);
  const progress = node.detail;
  if (!["active", "running"].includes(state.toLowerCase()) || !progress?.probe_state) return state;
  const count = progress.runs_since_transition ?? 0;
  return `${progress.probe_state} ×${count}`;
}
