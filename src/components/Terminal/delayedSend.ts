import { invoke } from "../../lib/bridge";

export type DelayedMessageState = "queued" | "stashed" | "sending" | "failed";

export interface RecurrenceRule {
  everyMs: number;
  untilMs?: number | null;
  count?: number | null;
}

export interface DelayedMessage {
  id: string;
  tabId: string;
  workspaceId?: string | null;
  text: string;
  scheduledAtMs: number;
  createdAtMs: number;
  state: DelayedMessageState;
  error?: string | null;
  sessionIdAtCreate?: string | null;
  currentSessionId?: string | null;
  sessionChanged: boolean;
  recurrence?: RecurrenceRule | null;
  seriesStartAtMs?: number | null;
  tickIndex?: number;
  skippedTicks?: number;
  teamId?: string | null;
  lastTickUnconfirmed?: boolean;
}

export async function listDelayedMessages(tabId: string): Promise<DelayedMessage[]> {
  // A remote peer may answer null; never let it reach callers as a non-array.
  return (await invoke<DelayedMessage[] | null>("delayed_send_list", { tabId })) ?? [];
}

export function createDelayedMessage(
  tabId: string,
  text: string,
  scheduledAtMs: number | null,
  recurrence?: RecurrenceRule,
): Promise<DelayedMessage> {
  return invoke<DelayedMessage>("delayed_send_create", {
    tabId,
    text,
    scheduledAtMs: scheduledAtMs ?? undefined,
    recurrence,
  });
}

export function updateDelayedMessage(
  id: string,
  patch: { text?: string; scheduledAtMs?: number; stash?: boolean; recurrence?: RecurrenceRule },
): Promise<DelayedMessage> {
  return invoke<DelayedMessage>("delayed_send_update", {
    id,
    text: patch.text,
    scheduledAtMs: patch.scheduledAtMs,
    stash: patch.stash,
    recurrence: patch.recurrence,
  });
}

export function deleteDelayedMessage(id: string): Promise<DelayedMessage> {
  return invoke<DelayedMessage>("delayed_send_delete", { id });
}

export function sendDelayedMessageNow(id: string): Promise<void> {
  return invoke<void>("delayed_send_send_now", { id });
}

export function toLocalInputValue(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function parseLocalInputValue(value: string): number | null {
  if (!value.trim()) return null;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : null;
}

export function formatScheduledTime(ms: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

export function formatClock(ms: number): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

export function formatRelative(ms: number, nowMs: number = Date.now()): string {
  const diff = ms - nowMs;
  const past = diff < 0;
  const abs = Math.abs(diff);
  const mins = Math.round(abs / 60_000);
  let body: string;
  if (mins < 1) body = "<1 min";
  else if (mins < 60) body = `${mins} min`;
  else if (mins < 60 * 24) {
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    body = m ? `${h}h ${m}m` : `${h}h`;
  } else {
    const days = Math.round(mins / (60 * 24));
    body = `${days}d`;
  }
  return past ? `${body} ago` : `in ${body}`;
}

export interface QuickPreset {
  label: string;
  resolve: (nowMs: number) => number;
}

function atHour(nowMs: number, hour: number, dayOffset: number): number {
  const date = new Date(nowMs);
  date.setDate(date.getDate() + dayOffset);
  date.setHours(hour, 0, 0, 0);
  return date.getTime();
}

export const QUICK_PRESETS: QuickPreset[] = [
  { label: "5m", resolve: (now) => now + 5 * 60_000 },
  { label: "10m", resolve: (now) => now + 10 * 60_000 },
  { label: "15m", resolve: (now) => now + 15 * 60_000 },
  { label: "30m", resolve: (now) => now + 30 * 60_000 },
  { label: "1h", resolve: (now) => now + 60 * 60_000 },
  { label: "3h", resolve: (now) => now + 3 * 60 * 60_000 },
  {
    label: "Tonight",
    resolve: (now) => {
      const at = atHour(now, 20, 0);
      return at > now ? at : atHour(now, 20, 1);
    },
  },
  { label: "Tomorrow", resolve: (now) => atHour(now, 9, 1) },
];
