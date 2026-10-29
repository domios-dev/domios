import { listen } from "../../lib/bridge";
import { localFetch } from "../../lib/webApi";

export type NeighborDeliveryStatus = "pending" | "delivered" | "not_delivered";
export type NeighborListenerState = "connecting" | "receiving" | "reconnecting" | "stopped";
export interface ReceiverState { receiving_here: boolean; receiver_machine_label: string | null; receiving_enabled: boolean; revision: number; source?: "account" | "friend" }
export type NeighborListenerEvent = { friendship_id: string; status: NeighborListenerState } | (ReceiverState & { friendship_id?: string; friend_receivers_version?: number }) | { receiver_unavailable: true; friendship_id?: string; friend_receivers_version?: number };
export type DeliveryRefusal = "receiver_moved" | "key_revoked" | "neighbor_daily_limit" | "rate_limited" | "aggregate_limited" | "aggregate_daily_limited" | "friends_locked" | "recipient_locked" | "recipient_not_receiving" | "recipient_paused";
const DELIVERY_REFUSALS: readonly string[] = ["receiver_moved", "key_revoked", "neighbor_daily_limit", "rate_limited", "aggregate_limited", "aggregate_daily_limited", "friends_locked", "recipient_locked", "recipient_not_receiving", "recipient_paused"];
export interface DeliveryResult {
  status: NeighborDeliveryStatus;
  reason?: DeliveryRefusal;
  /** Daily limit metadata, and how long hourly or overall limits ask to wait. */
  limit?: number;
  resets_at?: number;
  retry_after_seconds?: number;
}
export interface ChannelStatus extends ReceiverState {
  machine_label?: string;
  friend_receivers?: Record<string, ReceiverState>;
  friend_receivers_version?: number;
  listeners?: Record<string, NeighborListenerState>;
  enrolled: boolean;
  bindings: Record<string, string>;
}
/** Off until the daemon's permission accepts and enforces workspaces: the editor hides them and saves never send them. */
export const PERMISSION_WORKSPACES = false as boolean;

/** "none" is typed but no provider can honour it yet, so the editor never offers it; reads fall back to read only. */
export type MachineAccess = "none" | "read_only" | "folder" | "full";
export interface NeighborPermission {
  allowed: boolean;
  scope: string;
  allow_tab_requests: boolean;
  /** Off by default: their html and diagrams stay inert code. */
  allow_interactive_content?: boolean;
  /** On by default: their agents may ask yours about what your memory holds. */
  allow_memory?: boolean;
  /** Only these memory areas; null shares every area that is not secret. */
  memory_areas?: string[] | null;
  /** Only these workspaces; null, the default, grants no workspace at all. */
  workspaces?: string[] | null;
  /** On by default: their agents may ask for files. */
  allow_files?: boolean;
  /** Where their concierge works; null means the friend's own folder. */
  folder?: string | null;
  /** How far their concierge may act on this machine; read only unless the owner widens it. */
  machine_access?: MachineAccess;
  /** Read-only: the folder actually in effect, and how much of it the platform enforces. */
  effective_folder?: string;
  folder_enforcement?: { writes: "provider_sandbox" | "pad_only" | "unrestricted" | "advisory"; reads: "not_confined"; network?: "disabled" | "not_confined"; note: string };
}
export interface NeighborReceipt {
  message_id: string;
  status: NeighborDeliveryStatus;
  reason?: DeliveryRefusal;
  submission?: "unconfirmed";
}
export interface NeighborRuntimeClient {
  status(signal?: AbortSignal): Promise<ChannelStatus>;
  enroll(revision: number): Promise<void>;
  receiveFriend(friendship: string, revision: number, operation?: "set" | "reset"): Promise<void>;
  rotate(): Promise<void>;
  permission(friendship: string, signal?: AbortSignal): Promise<NeighborPermission>;
  send(friendship: string, body: string): Promise<NeighborReceipt>;
  deliveryStatus(id: string): Promise<DeliveryResult>;
  retry(friendship: string): Promise<void>;
  subscribe(handler: (event: NeighborListenerEvent) => void): Promise<() => void>;
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}
export function deliveryStatus(value: unknown): NeighborDeliveryStatus {
  if (value !== "pending" && value !== "delivered" && value !== "not_delivered") throw new Error("Invalid Neighbor delivery status");
  return value;
}
export function deliveryResult(value: unknown): DeliveryResult {
  if (!record(value)) throw new Error("Invalid Neighbor delivery result");
  const count = (field: unknown) => typeof field === "number" && Number.isFinite(field) && field >= 0;
  return { status: deliveryStatus(value.status),
    ...(typeof value.reason === "string" && DELIVERY_REFUSALS.includes(value.reason) ? { reason: value.reason as DeliveryRefusal } : {}),
    ...(count(value.limit) ? { limit: value.limit as number } : {}),
    ...(count(value.resets_at) ? { resets_at: value.resets_at as number } : {}),
    ...(count(value.retry_after_seconds) ? { retry_after_seconds: value.retry_after_seconds as number } : {}) };
}

export function receipt(value: unknown): NeighborReceipt {
  if (!record(value) || typeof value.message_id !== "string" || !/^[0-9a-f]{32}$/.test(value.message_id)) throw new Error("Invalid Neighbor receipt");
  return { message_id: value.message_id, ...deliveryResult(value),
    ...(value.submission === "unconfirmed" ? { submission: "unconfirmed" as const } : {}) };
}

export async function runtimeCall(command: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors/runtime", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command), signal,
  });
  if (!response.ok) {
    const detail = await response.text();
    const failure: unknown = (() => {
      try { return JSON.parse(detail); } catch { return null; }
    })();
    const reason = record(failure) && (["feature_unsupported", "key_limit", "key_not_found", "key_revoked", "invalid_key", "entitlements_unavailable", "message_too_large", "tab_requests_not_allowed", "neighbor_permission_denied", "friendship_ended", "neighbor_forbidden", "session_revoked", "operation_id_conflict", "neighbors_disabled", "installation_unverified", "receiver_elsewhere", "receiver_revision_stale", "not_receiver", "recipient_not_receiving", "reply_target_stopped", "receiving_elsewhere", "receiver_not_chosen", "receiver_status_unknown", "receivers_present"] as const).find(reason => reason === failure.code);
    const message = (record(failure) && typeof failure.message === "string" ? failure.message : detail);
    const limit = record(failure) && typeof failure.limit === "number" ? failure.limit : undefined;
    // A sending limit (HTTP 429) names its delivery reason and reset, which the conversation explains like a refused receipt.
    const refusal = record(failure) && typeof failure.reason === "string" && DELIVERY_REFUSALS.includes(failure.reason)
      ? { ...deliveryResult({ status: "not_delivered", reason: failure.reason, limit: failure.limit, resets_at: failure.resets_at, retry_after_seconds: failure.retry_after_seconds }) }
      : undefined;
    const active = record(failure) && typeof failure.active === "number" && Number.isSafeInteger(failure.active) && failure.active >= 0 ? failure.active : undefined;
    throw new NeighborRuntimeFailure(reason || "unavailable", message.trim() || `Neighbor operation failed (HTTP ${response.status})`, limit, refusal, active);
  }
  return response.json();
}

export type ReceiverRefusal = "feature_unsupported" | "key_limit" | "key_not_found" | "key_revoked" | "invalid_key" | "entitlements_unavailable" | "message_too_large" | "tab_requests_not_allowed" | "neighbor_permission_denied" | "friendship_ended" | "neighbor_forbidden" | "session_revoked" | "operation_id_conflict" | "neighbors_disabled" | "installation_unverified" | "receiver_elsewhere" | "receiver_revision_stale" | "not_receiver" | "recipient_not_receiving" | "reply_target_stopped"
  /** Bind, Apply, ensure and create on a machine that is not this friendship's effective receiver (HTTP 409). */
  | "receiving_elsewhere" | "receiver_not_chosen" | "receiver_status_unknown" | "receivers_present" | "unavailable";
/** Proven pre-send refusals that sending the same thing again cannot change, so no Retry is offered:
 *  the owner stopped the request this replies to. */
export const FINAL_REFUSALS: ReadonlySet<ReceiverRefusal> = new Set(["reply_target_stopped"]);
export class NeighborRuntimeFailure extends Error {
  constructor(readonly reason: ReceiverRefusal, detail?: string, readonly limit?: number, readonly refusal?: DeliveryResult, readonly active?: number) { super(detail || reason); }
}
export function receiverState(value: unknown): value is ReceiverState {
  return record(value) && typeof value.receiving_here === "boolean" && typeof value.receiving_enabled === "boolean"
    && (value.receiver_machine_label === null || typeof value.receiver_machine_label === "string")
    && typeof value.revision === "number" && Number.isSafeInteger(value.revision) && value.revision >= 0
    && (value.source === undefined || value.source === "account" || value.source === "friend");
}

export const neighborRuntimeClient: NeighborRuntimeClient = {
  retry: async friendship => { await runtimeCall({ operation: "retry", friendship_id: friendship }); },
  subscribe: handler => listen<NeighborListenerEvent>("neighbor-runtime-status", event => {
    const value = event.payload;
    if (receiverState(value) || ("receiver_unavailable" in value && value.receiver_unavailable === true)
      || ("friendship_id" in value && "status" in value && typeof value.friendship_id === "string" && ["connecting", "receiving", "reconnecting", "stopped"].includes(value.status))) handler(value);
  }),
  status: async signal => {
    const value = await runtimeCall({ operation: "status" }, signal);
    if (!record(value) || !receiverState(value) || typeof value.enrolled !== "boolean" || !record(value.bindings)
      || !Object.values(value.bindings).every(id => typeof id === "string")) throw new Error("Invalid Neighbor channel status");
    if (value.friend_receivers !== undefined && (!record(value.friend_receivers) || !Object.values(value.friend_receivers).every(receiverState)
      || typeof value.friend_receivers_version !== "number" || !Number.isSafeInteger(value.friend_receivers_version) || value.friend_receivers_version < 0)) throw new Error("Invalid Neighbor friend receiver status");
    return value as unknown as ChannelStatus;
  },
  enroll: async revision => { await runtimeCall({ operation: "enroll", takeover_confirmed: true, expected_receiver_revision: revision }); },
  receiveFriend: async (friendship, revision, operation = "set") => { await runtimeCall({ operation: "set_friend_receiver", friendship_id: friendship, receiver_operation: operation, takeover_confirmed: true, expected_receiver_revision: revision }); },
  rotate: async () => { await runtimeCall({ operation: "rotate" }); },
  permission: async (friendship, signal) => {
    const value = await runtimeCall({ operation: "permission", friendship_id: friendship }, signal);
    if (!record(value) || typeof value.allowed !== "boolean" || typeof value.scope !== "string" || typeof value.allow_tab_requests !== "boolean") throw new Error("Invalid Neighbor permission");
    return value as unknown as NeighborPermission;
  },
  send: async (friendship, body) => receipt(await runtimeCall({ operation: "send", friendship_id: friendship, kind: "question", body, in_reply_to: null })),
  deliveryStatus: async id => {
    const result = await runtimeCall({ operation: "delivery_status", message_id: id });
    if (!record(result)) throw new Error("Invalid Neighbor delivery status");
    return deliveryResult(result);
  },
};
