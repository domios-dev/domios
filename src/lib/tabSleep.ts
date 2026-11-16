import type { Tab } from "./types";

export function asleepLabel(tab: Pick<Tab, "slept_at_ms">, nowMs: number): string {
  if (tab.slept_at_ms == null || !Number.isFinite(tab.slept_at_ms)) return "Asleep";
  const hours = Math.floor(Math.max(0, nowMs - tab.slept_at_ms) / 3_600_000);
  return `Asleep, idle ${hours}h`;
}

export function hasSleepingTranscript(tab: Partial<Tab>): boolean {
  const session = tab.agent_session_id || (tab.backing?.kind === "chat_backend" && tab.backing.session_id);
  return tab.slept_at_ms != null && Boolean(session);
}
