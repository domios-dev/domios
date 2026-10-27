import { listen } from "../../lib/bridge";
import { deliveryStatus, runtimeCall, type NeighborDeliveryStatus, type NeighborPermission } from "./neighborRuntimeClient";

export type HistoryAuthor = "me" | "friend" | { agent: string; name: string };

export type HistoryKind = "question" | "answer" | "refusal" | "tab_request";

export interface HistoryEntry {
  direction: "in" | "out";
  kind: HistoryKind;
  in_reply_to?: string;
  /** Incoming only: how your side handled it, and why when it was refused. */
  outcome?: "refused" | "answered";
  reason?: string;
  /** Set on a message the daily limit refused: the limit and when it resets, in unix milliseconds. */
  limit?: number;
  resets_at?: number;
  /** Hourly or overall limits: how long until another send may go through. */
  retry_after_seconds?: number;
  author: HistoryAuthor;
  body: string;
  /** Unix milliseconds. */
  at: number;
  message_id: string;
  delivery_status?: NeighborDeliveryStatus;
  /** Outgoing only: whether the relay took the message at all. */
  submission?: "accepted" | "unconfirmed" | "refused";
}

export type NeighborNotify = "off" | "in_app" | "in_app_and_bridges";

export interface HistoryPage { entries: HistoryEntry[]; next_before: string | null; ended: boolean }

export interface NeighborConciergeClient {
  history(friendship: string, before?: string, signal?: AbortSignal): Promise<HistoryPage>;
  onHistoryChanged(handler: (friendship: string) => void): Promise<() => void>;
  onPermissionChanged(handler: (friendship: string) => void): Promise<() => void>;
  /** A null tab means the friendship no longer has a concierge here, for example after removal. */
  onAgentChanged(handler: (friendship: string, gatewayTab: string | null) => void): Promise<() => void>;
  setPermission(friendship: string, permission: NeighborPermission): Promise<void>;
  changeAgent(friendship: string, provider: string, model?: string): Promise<string>;
  setDailyLimit(friendship: string, dailyLimit: number | null): Promise<void>;
  onDailyLimitChanged(handler: (friendship: string) => void): Promise<() => void>;
  setNotify(friendship: string, notify: NeighborNotify): Promise<NeighborNotify>;
  onNotifyChanged(handler: (friendship: string, notify: NeighborNotify) => void): Promise<() => void>;
}

const KINDS: readonly HistoryKind[] = ["question", "answer", "refusal", "tab_request"];
// Reset times may arrive in seconds or milliseconds; anything before 2001 in milliseconds is seconds.
const unixMillis = (value: number) => value < 1e12 ? value * 1000 : value;
export const isNotify = (value: unknown): value is NeighborNotify => value === "off" || value === "in_app" || value === "in_app_and_bridges";
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

function author(value: unknown): HistoryAuthor {
  if (value === "me" || value === "friend") return value;
  if (record(value) && typeof value.agent === "string" && typeof value.name === "string") return { agent: value.agent, name: value.name };
  throw new Error("Invalid Neighbor history author");
}

export function parseHistory(value: unknown): HistoryPage {
  if (!record(value) || !Array.isArray(value.entries) || typeof value.ended !== "boolean"
    || (value.next_before !== null && typeof value.next_before !== "string")) throw new Error("Invalid Neighbor history");
  return {
    ended: value.ended,
    next_before: value.next_before,
    entries: value.entries.map(entry => {
      if (!record(entry) || (entry.direction !== "in" && entry.direction !== "out") || typeof entry.body !== "string"
        || !KINDS.includes(entry.kind as HistoryKind)
        || typeof entry.at !== "number" || typeof entry.message_id !== "string") throw new Error("Invalid Neighbor history entry");
      return { direction: entry.direction, kind: entry.kind as HistoryKind, author: author(entry.author), body: entry.body, at: entry.at, message_id: entry.message_id,
        ...(typeof entry.in_reply_to === "string" ? { in_reply_to: entry.in_reply_to } : {}),
        ...(entry.outcome === "refused" || entry.outcome === "answered" ? { outcome: entry.outcome } : {}),
        ...(typeof entry.reason === "string" ? { reason: entry.reason } : {}),
        ...(typeof entry.limit === "number" ? { limit: entry.limit } : {}),
        ...(typeof entry.resets_at === "number" ? { resets_at: unixMillis(entry.resets_at) } : {}),
        ...(typeof entry.retry_after_seconds === "number" ? { retry_after_seconds: entry.retry_after_seconds } : {}),
        ...(entry.submission === "accepted" || entry.submission === "unconfirmed" || entry.submission === "refused" ? { submission: entry.submission } : {}),
        ...(entry.delivery_status === undefined ? {} : { delivery_status: deliveryStatus(entry.delivery_status) }) };
    }),
  };
}

export const neighborConciergeClient: NeighborConciergeClient = {
  history: async (friendship, before, signal) => parseHistory(await runtimeCall({ operation: "history", friendship_id: friendship, ...(before ? { before } : {}) }, signal)),
  onHistoryChanged: handler => listen<{ friendship_id?: unknown }>("neighbor-history-changed", event => {
    if (typeof event.payload?.friendship_id === "string") handler(event.payload.friendship_id);
  }),
  onPermissionChanged: handler => listen<{ friendship_id?: unknown }>("neighbor-permission-changed", event => {
    if (typeof event.payload?.friendship_id === "string") handler(event.payload.friendship_id);
  }),
  onAgentChanged: handler => listen<{ friendship_id?: unknown; gateway_tab?: unknown }>("neighbor-agent-changed", event => {
    const tab = event.payload?.gateway_tab;
    if (typeof event.payload?.friendship_id === "string" && (tab === null || typeof tab === "string")) handler(event.payload.friendship_id, tab);
  }),
  setPermission: async (friendship, permission) => {
    // Only what the owner edits travels; the effective folder and its enforcement are the daemon's to report.
    const { allowed, scope, allow_tab_requests, allow_interactive_content, allow_memory, allow_files, folder, machine_access, memory_areas, workspaces } = permission;
    await runtimeCall({ operation: "set_permission", friendship_id: friendship,
      permission: { allowed, scope, allow_tab_requests, allow_interactive_content, allow_memory, allow_files, folder, machine_access, memory_areas, workspaces } });
  },
  setNotify: async (friendship, notify) => {
    const value = await runtimeCall({ operation: "set_notification_preference", friendship_id: friendship, notify });
    if (!record(value) || !isNotify(value.notify)) throw new Error("Invalid notification preference");
    return value.notify;
  },
  onNotifyChanged: handler => listen<{ friendship_id?: unknown; notify?: unknown }>("neighbor-notification-changed", event => {
    if (typeof event.payload?.friendship_id === "string" && isNotify(event.payload.notify)) handler(event.payload.friendship_id, event.payload.notify);
  }),
  setDailyLimit: async (friendship, dailyLimit) => { await runtimeCall({ operation: "set_daily_limit", friendship_id: friendship, daily_limit: dailyLimit }); },
  onDailyLimitChanged: handler => listen<{ friendship_id?: unknown }>("neighbor-daily-limit-changed", event => {
    if (typeof event.payload?.friendship_id === "string") handler(event.payload.friendship_id);
  }),
  changeAgent: async (friendship, provider, model) => {
    const value = await runtimeCall({ operation: "change_agent", friendship_id: friendship, provider, ...(model ? { model } : {}) });
    if (!record(value) || typeof value.gateway_tab !== "string") throw new Error("Invalid concierge change");
    return value.gateway_tab;
  },
};
