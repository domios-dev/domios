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
  author: HistoryAuthor;
  body: string;
  /** Unix milliseconds. */
  at: number;
  message_id: string;
  delivery_status?: NeighborDeliveryStatus;
}

export interface HistoryPage { entries: HistoryEntry[]; next_before: string | null; ended: boolean }

export interface NeighborConciergeClient {
  history(friendship: string, before?: string, signal?: AbortSignal): Promise<HistoryPage>;
  onHistoryChanged(handler: (friendship: string) => void): Promise<() => void>;
  onPermissionChanged(handler: (friendship: string) => void): Promise<() => void>;
  onAgentChanged(handler: (friendship: string, gatewayTab: string) => void): Promise<() => void>;
  setPermission(friendship: string, permission: NeighborPermission): Promise<void>;
  changeAgent(friendship: string, provider: string, model?: string): Promise<string>;
}

const KINDS: readonly HistoryKind[] = ["question", "answer", "refusal", "tab_request"];
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
    if (typeof event.payload?.friendship_id === "string" && typeof event.payload.gateway_tab === "string") handler(event.payload.friendship_id, event.payload.gateway_tab);
  }),
  setPermission: async (friendship, permission) => { await runtimeCall({ operation: "set_permission", friendship_id: friendship, permission }); },
  changeAgent: async (friendship, provider, model) => {
    const value = await runtimeCall({ operation: "change_agent", friendship_id: friendship, provider, ...(model ? { model } : {}) });
    if (!record(value) || typeof value.gateway_tab !== "string") throw new Error("Invalid concierge change");
    return value.gateway_tab;
  },
};
