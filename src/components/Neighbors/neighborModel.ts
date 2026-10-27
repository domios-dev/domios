export interface Neighbor {
  uid: string;
  displayName: string | null;
  photoURL: string | null;
  friendship_id: string;
  inbound_per_hour: number;
  outbound_per_hour: number;
  since: number;
}

export interface NeighborRequest {
  uid: string;
  displayName: string | null;
  photoURL: string | null;
  request_id: string;
  created_at: number;
}

export interface NeighborList {
  schema_version: 1;
  access: "paid" | "locked";
  friends: Neighbor[];
  incoming: NeighborRequest[];
}

export type NeighborCommand =
  | { operation: "list" }
  | { operation: "request"; email: string }
  | { operation: "respond"; request_id: string; accept: boolean }
  | { operation: "remove"; friendship_id: string }
  | { operation: "set_limit"; friendship_id: string; per_hour: number };

export const neighborName = (value: Pick<Neighbor, "displayName" | "uid">): string => value.displayName || value.uid;

const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 256;
const nullableText = (value: unknown): boolean => value === null || typeof value === "string";
const count = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const profile = (value: unknown): value is Record<string, unknown> => record(value) && text(value.uid) && nullableText(value.displayName) && nullableText(value.photoURL);

export function parseNeighbors(value: unknown): NeighborList {
  if (!record(value) || value.schema_version !== 1 || !["paid", "locked"].includes(String(value.access))
    || !Array.isArray(value.friends) || !Array.isArray(value.incoming) || value.friends.length > 200 || value.incoming.length > 200
    || !value.friends.every(friend => profile(friend) && text(friend.friendship_id) && count(friend.since)
      && count(friend.inbound_per_hour) && count(friend.outbound_per_hour))
    || !value.incoming.every(request => profile(request) && text(request.request_id) && count(request.created_at))) {
    throw new Error("Invalid Neighbors response");
  }
  return value as unknown as NeighborList;
}

export function validNeighborLimit(value: number): boolean {
  return Number.isInteger(value) && value >= 1 && value <= 200;
}
