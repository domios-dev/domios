import { neighborFetch } from "./neighborFetch";
import { recordProductEvent } from "../../lib/productAnalytics";
import { parseNeighbors, type NeighborCommand, type NeighborList } from "./neighborModel";

export interface NeighborClient {
  list(signal?: AbortSignal, refresh?: boolean): Promise<NeighborList>;
  apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<void>;
}

const localUnavailable = "Neighbors on this device isn’t ready yet.";

function listFailure(message: string, status: number): string {
  if (message === "Sign in with a hosted account to use Neighbors") return message;
  if (message === "Sign in to manage Neighbors" || message.startsWith("session_revoked:")
    || message.startsWith("Neighbor HTTP 401 ")) return "Sign in to use Neighbors.";
  if (message.startsWith("neighbors_disabled:")) return "Neighbors is off.";
  if (message.startsWith("neighbor_local_fence:") || message.startsWith("key_revoked:")
    || message.startsWith("Account changed while ") || message === "Neighbor account changed"
    || message === "Neighbor gateway unavailable" || message === "Neighbor cursor storage unavailable"
    || status === 401 || status === 502 || status === 504) return localUnavailable;
  if (message === "Neighbor transport unavailable" || message === "Neighbor callable unavailable"
    || /^Neighbor HTTP 5\d\d /.test(message)) return "The hosted Neighbors service is temporarily unavailable.";
  return "Couldn’t load Neighbors.";
}

async function call(command: NeighborCommand | { operation: "refresh" }, signal?: AbortSignal): Promise<unknown> {
  const listing = command.operation === "list" || command.operation === "refresh";
  const response = await neighborFetch("/api/account/neighbors", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(command), signal,
  }).catch((failure: unknown) => {
    if (!listing || signal?.aborted) throw failure;
    throw new Error(localUnavailable, { cause: failure });
  });
  if (!response.ok) {
    const message = await response.text();
    throw new Error(listing ? listFailure(message.trim(), response.status) : message);
  }
  return response.json();
}

export const neighborClient: NeighborClient = {
  list: async (signal, refresh = false) => parseNeighbors(await call({ operation: refresh ? "refresh" : "list" }, signal)),
  apply: async command => {
    await call(command);
    if (command.operation === "respond" && command.accept) void recordProductEvent({ event_type: "friend_added", event_properties: { count: 1 } });
  },
};
