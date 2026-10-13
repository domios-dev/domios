import type { AgentRuntimeState } from "./agentRuntimeState";
import type { TabActivity } from "./tabActivity";
import { formatDuration } from "./timeLabels";

const validTime = (value: number | null | undefined): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= 0;

export function runtimeActivityLabel(
  label: string,
  kind: TabActivity["kind"],
  runtime: AgentRuntimeState | null | undefined,
  now: number,
): string {
  if (runtime?.state !== "loaded") return label;
  if (kind === "working" && validTime(runtime.active_turn_started_at_ms) && validTime(now)) {
    return [label, `Elapsed ${formatDuration(now - runtime.active_turn_started_at_ms)}`].filter(Boolean).join(" · ");
  }
  if (kind === "idle" && validTime(runtime.last_turn_duration_ms)) {
    return [label, `Last turn ${formatDuration(runtime.last_turn_duration_ms)}`].filter(Boolean).join(" · ");
  }
  return label;
}
