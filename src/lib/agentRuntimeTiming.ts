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
  observedLastTurnMs?: number,
): string {
  const loaded = runtime?.state === "loaded" ? runtime : null;
  if (kind === "working" && loaded && validTime(loaded.active_turn_started_at_ms) && validTime(now)) {
    return [label, `Elapsed ${formatDuration(now - loaded.active_turn_started_at_ms)}`].filter(Boolean).join(" · ");
  }
  const lastTurnMs = loaded && validTime(loaded.last_turn_duration_ms) ? loaded.last_turn_duration_ms : observedLastTurnMs;
  if (kind === "idle" && validTime(lastTurnMs)) {
    return [label, `Last turn ${formatDuration(lastTurnMs)}`].filter(Boolean).join(" · ");
  }
  return label;
}
