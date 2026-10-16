import type { AppConfig, Tab } from "./types";

export function tabSleepSettings(config: Pick<AppConfig, "tab_sleep">) {
  return { enabled: config.tab_sleep?.enabled ?? true, idle_minutes: config.tab_sleep?.idle_minutes ?? 60 };
}

export function sleepThresholdMinutes(value: string): number | null {
  if (!value.trim()) return null;
  const minutes = Number(value);
  return Number.isSafeInteger(minutes) && minutes > 0 ? minutes : null;
}

export function asleepLabel(tab: Pick<Tab, "slept_at_ms">, nowMs: number): string {
  if (tab.slept_at_ms == null || !Number.isFinite(tab.slept_at_ms)) return "Asleep";
  const hours = Math.floor(Math.max(0, nowMs - tab.slept_at_ms) / 3_600_000);
  return `Asleep, idle ${hours}h`;
}

export function hasSleepingTranscript(tab: Partial<Tab>): boolean {
  const session = tab.agent_session_id || (tab.backing?.kind === "chat_backend" && tab.backing.session_id);
  return tab.slept_at_ms != null && Boolean(session);
}
