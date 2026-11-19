import type { Tab } from "./types";

export function asleepDuration(tab: Pick<Tab, "slept_at_ms">, nowMs: number): string | null {
  if (tab.slept_at_ms == null || !Number.isFinite(tab.slept_at_ms)) return null;
  const hours = Math.floor(Math.max(0, nowMs - tab.slept_at_ms) / 3_600_000);
  return `${hours}h`;
}

export function asleepLabel(tab: Pick<Tab, "slept_at_ms">, nowMs: number): string {
  const duration = asleepDuration(tab, nowMs);
  return duration ? `Asleep, idle ${duration}` : "Asleep";
}

export function hasSleepingTranscript(tab: Partial<Tab>): boolean {
  const session = tab.agent_session_id || (tab.backing?.kind === "chat_backend" && tab.backing.session_id);
  return tab.slept_at_ms != null && Boolean(session);
}
