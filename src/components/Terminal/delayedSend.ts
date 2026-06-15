import { invoke } from "../../lib/bridge";

export type DelayedMessageState = "queued" | "sending" | "failed";

export interface DelayedMessage {
  id: string;
  paneId: string;
  workspaceId?: string | null;
  text: string;
  scheduledAtMs: number;
  createdAtMs: number;
  state: DelayedMessageState;
  error?: string | null;
  sessionIdAtCreate?: string | null;
  currentSessionId?: string | null;
  sessionChanged: boolean;
}

export function listDelayedMessages(paneId: string): Promise<DelayedMessage[]> {
  return invoke<DelayedMessage[]>("delayed_send_list", { paneId });
}

export function createDelayedMessage(
  paneId: string,
  text: string,
  scheduledAtMs: number,
): Promise<DelayedMessage> {
  return invoke<DelayedMessage>("delayed_send_create", { paneId, text, scheduledAtMs });
}

export function updateDelayedMessage(
  id: string,
  patch: { text?: string; scheduledAtMs?: number },
): Promise<DelayedMessage> {
  return invoke<DelayedMessage>("delayed_send_update", {
    id,
    text: patch.text,
    scheduledAtMs: patch.scheduledAtMs,
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
