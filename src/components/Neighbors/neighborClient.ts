import { localFetch } from "../../lib/webApi";
import { recordProductEvent } from "../../lib/productAnalytics";
import { parseNeighbors, type NeighborCommand, type NeighborList } from "./neighborModel";

export interface NeighborClient {
  list(signal?: AbortSignal, refresh?: boolean): Promise<NeighborList>;
  apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<void>;
}

async function call(command: NeighborCommand | { operation: "refresh" }, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(command), signal,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

export const neighborClient: NeighborClient = {
  list: async (signal, refresh = false) => parseNeighbors(await call({ operation: refresh ? "refresh" : "list" }, signal)),
  apply: async command => {
    await call(command);
    if (command.operation === "respond" && command.accept) void recordProductEvent({ event_type: "friend_added", event_properties: { count: 1 } });
  },
};
