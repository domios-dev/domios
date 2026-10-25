import { localFetch } from "../../lib/webApi";

export type NeighborDeliveryStatus = "pending" | "delivered" | "not_delivered";
export interface ChannelStatus {
  enrolled: boolean;
  bindings: Record<string, string>;
}
export interface NeighborPermission {
  allowed: boolean;
  scope: string;
  allow_tab_requests: boolean;
}
export interface NeighborReceipt {
  message_id: string;
  status: NeighborDeliveryStatus;
  submission?: "unconfirmed";
}
export interface NeighborRuntimeClient {
  status(signal?: AbortSignal): Promise<ChannelStatus>;
  enroll(): Promise<void>;
  rotate(): Promise<void>;
  permission(friendship: string, signal?: AbortSignal): Promise<NeighborPermission>;
  send(friendship: string, body: string): Promise<NeighborReceipt>;
  deliveryStatus(id: string): Promise<NeighborDeliveryStatus>;
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}
export function deliveryStatus(value: unknown): NeighborDeliveryStatus {
  if (value !== "pending" && value !== "delivered" && value !== "not_delivered") throw new Error("Invalid Neighbor delivery status");
  return value;
}
export function receipt(value: unknown): NeighborReceipt {
  if (!record(value) || typeof value.message_id !== "string" || !/^[0-9a-f]{32}$/.test(value.message_id)) throw new Error("Invalid Neighbor receipt");
  return { message_id: value.message_id, status: deliveryStatus(value.status),
    ...(value.submission === "unconfirmed" ? { submission: "unconfirmed" as const } : {}) };
}

async function call(command: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors/runtime", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command), signal,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

export const neighborRuntimeClient: NeighborRuntimeClient = {
  status: async signal => {
    const value = await call({ operation: "status" }, signal);
    if (!record(value) || typeof value.enrolled !== "boolean" || !record(value.bindings)
      || !Object.values(value.bindings).every(id => typeof id === "string")) throw new Error("Invalid Neighbor channel status");
    return value as unknown as ChannelStatus;
  },
  enroll: async () => { await call({ operation: "enroll", takeover_confirmed: true }); },
  rotate: async () => { await call({ operation: "rotate" }); },
  permission: async (friendship, signal) => {
    const value = await call({ operation: "permission", friendship_id: friendship }, signal);
    if (!record(value) || typeof value.allowed !== "boolean" || typeof value.scope !== "string" || typeof value.allow_tab_requests !== "boolean") throw new Error("Invalid Neighbor permission");
    return value as unknown as NeighborPermission;
  },
  send: async (friendship, body) => receipt(await call({ operation: "send", friendship_id: friendship, kind: "question", body, in_reply_to: null })),
  deliveryStatus: async id => {
    const result = await call({ operation: "delivery_status", message_id: id });
    if (!record(result)) throw new Error("Invalid Neighbor delivery status");
    return deliveryStatus(result.status);
  },
};
