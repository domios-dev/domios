import { listen } from "../../lib/bridge";
import { localFetch } from "../../lib/webApi";
import { deliveryStatus, PERMISSION_WORKSPACES, runtimeCall, type NeighborPermissionDraft } from "./neighborRuntimeClient";

export type HistoryAuthor = import("../../../gen/NeighborHistoryAuthor").NeighborHistoryAuthor;

export type HistoryKind = import("../../../gen/NeighborHistoryKind").NeighborHistoryKind;

export type HistoryEntry = Omit<import("../../../gen/NeighborHistoryEntry").NeighborHistoryEntry, "sender_uid">;

export type FlagCategory = import("../../../gen/FlagCategory").FlagCategory;
const FLAG_CATEGORIES: readonly string[] = ["suspicious_instructions", "private_data_request", "malicious_link", "spam", "other"];
/** `at` is unix milliseconds. */
export type MessageFlag = import("../../../gen/NeighborMessageFlag").NeighborMessageFlag;

function flags(value: unknown): MessageFlag[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap(flag => record(flag) && FLAG_CATEGORIES.includes(flag.category as string) && typeof flag.reason === "string"
    && (flag.by === "concierge" || flag.by === "tab" || flag.by === "owner") && typeof flag.at === "number"
    ? [{ category: flag.category as FlagCategory, reason: flag.reason, by: flag.by, at: unixMillis(flag.at) }] : []);
}

/** How a concierge runs beyond its provider and model: reasoning effort (a ReasoningEffortInfo id) and speed tier. */
export interface ConciergeLaunchOptions { reasoning_effort?: string; service_tier?: string }

/** Marks a message from them; their concierge and agents see the flag through history. */
export async function flagNeighborMessage(friendship: string, messageId: string, category: FlagCategory, reason: string): Promise<void> {
  await runtimeCall({ operation: "flag_message", friendship_id: friendship, message_id: messageId, category, reason });
}

export type HandlingState = import("../../../gen/NeighborHandlingState").NeighborHandlingState;
const HANDLING_STATES: readonly string[] = ["delivered_to_concierge", "working", "answered", "refused", "no_reply", "failed", "waiting", "stopped"];

export type NeighborNotify = import("../../../gen/NeighborNotify").NeighborNotify;

export type HistoryPage = Omit<import("../../../gen/NeighborHistoryPage").NeighborHistoryPage, "entries"> & { entries: HistoryEntry[] };

export interface NeighborConciergeClient {
  history(friendship: string, before?: string, signal?: AbortSignal): Promise<HistoryPage>;
  onHistoryChanged(handler: (friendship: string) => void): Promise<() => void>;
  onPermissionChanged(handler: (friendship: string) => void): Promise<() => void>;
  /** A null tab means the friendship no longer has a concierge here, for example after removal. */
  onAgentChanged(handler: (friendship: string, gatewayTab: string | null) => void): Promise<() => void>;
  setPermission(friendship: string, permission: NeighborPermissionDraft): Promise<void>;
  /** Omitted options mean the provider's defaults; `service_tier` "default" is the standard speed. */
  changeAgent(friendship: string, provider: string, model?: string, options?: ConciergeLaunchOptions): Promise<string>;
  setDailyLimit(friendship: string, dailyLimit: number | null): Promise<void>;
  onDailyLimitChanged(handler: (friendship: string) => void): Promise<() => void>;
  setNotify(friendship: string, notify: NeighborNotify): Promise<NeighborNotify>;
  onNotifyChanged(handler: (friendship: string, notify: NeighborNotify) => void): Promise<() => void>;
  /** Pausing keeps the friendship but stops their messages until you resume; resolves to the saved state. */
  setPaused(friendship: string, paused: boolean): Promise<boolean>;
  onPausedChanged(handler: (friendship: string, paused: boolean) => void): Promise<() => void>;
  /** Stops this friend's concierge turn; the entries it was handling become stopped. */
  stopConcierge(friendship: string): Promise<void>;
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
        ...(HANDLING_STATES.includes(entry.handling_state as string) ? { handling_state: entry.handling_state as HandlingState } : {}),
        ...(typeof entry.handling_reason === "string" ? { handling_reason: entry.handling_reason } : {}),
        ...(typeof entry.handling_updated_at === "number" ? { handling_updated_at: entry.handling_updated_at } : {}),
        ...(typeof entry.delivered_at === "number" ? { delivered_at: unixMillis(entry.delivered_at) } : {}),
        ...(typeof entry.read_at === "number" ? { read_at: unixMillis(entry.read_at) } : {}),
        ...(flags(entry.flags).length > 0 ? { flags: flags(entry.flags) } : {}),
        ...(entry.delivery_status === undefined ? {} : { delivery_status: deliveryStatus(entry.delivery_status) }) };
    }),
  };
}

/** Tells the runtime you have seen their messages up to this one; only the open, visible conversation calls it, never a history read. */
export async function markNeighborRead(friendship: string, throughMessageId: string): Promise<void> {
  const response = await localFetch("/api/account/neighbors/read", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ friendship_id: friendship, through_message_id: throughMessageId }),
  });
  if (!response.ok) throw new Error(`Read receipt failed (HTTP ${response.status})`);
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
    const { allowed, scope, allow_tab_requests, allow_interactive_content, allow_memory, allow_files, folder, machine_access, memory_areas, memory_write_areas = [], workspaces } = permission;
    await runtimeCall({ operation: "set_permission", friendship_id: friendship,
      permission: { allowed, scope, allow_tab_requests, allow_interactive_content, allow_memory, allow_files, folder, machine_access, memory_areas, memory_write_areas,
        ...(PERMISSION_WORKSPACES ? { workspaces } : {}) } });
  },
  setNotify: async (friendship, notify) => {
    const value = await runtimeCall({ operation: "set_notification_preference", friendship_id: friendship, notify });
    if (!record(value) || !isNotify(value.notify)) throw new Error("Invalid notification preference");
    return value.notify;
  },
  onNotifyChanged: handler => listen<{ friendship_id?: unknown; notify?: unknown }>("neighbor-notification-changed", event => {
    if (typeof event.payload?.friendship_id === "string" && isNotify(event.payload.notify)) handler(event.payload.friendship_id, event.payload.notify);
  }),
  setPaused: async (friendship, paused) => {
    const value = await runtimeCall({ operation: paused ? "pause_friend" : "resume_friend", friendship_id: friendship });
    return record(value) && typeof value.paused === "boolean" ? value.paused : paused;
  },
  onPausedChanged: handler => listen<{ friendship_id?: unknown; paused?: unknown }>("neighbor-friend-paused", event => {
    if (typeof event.payload?.friendship_id === "string" && typeof event.payload.paused === "boolean") handler(event.payload.friendship_id, event.payload.paused);
  }),
  stopConcierge: async friendship => { await runtimeCall({ operation: "stop_concierge", friendship_id: friendship }); },
  setDailyLimit: async (friendship, dailyLimit) => { await runtimeCall({ operation: "set_daily_limit", friendship_id: friendship, daily_limit: dailyLimit }); },
  onDailyLimitChanged: handler => listen<{ friendship_id?: unknown }>("neighbor-daily-limit-changed", event => {
    if (typeof event.payload?.friendship_id === "string") handler(event.payload.friendship_id);
  }),
  changeAgent: async (friendship, provider, model, options) => {
    const value = await runtimeCall({ operation: "change_agent", friendship_id: friendship, provider, ...(model ? { model } : {}),
      ...(options?.reasoning_effort ? { reasoning_effort: options.reasoning_effort } : {}), ...(options?.service_tier ? { service_tier: options.service_tier } : {}) });
    if (!record(value) || typeof value.gateway_tab !== "string") throw new Error("Invalid concierge change");
    return value.gateway_tab;
  },
};
