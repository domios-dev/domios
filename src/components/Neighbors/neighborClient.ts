import { localFetch } from "../../lib/webApi";
import { parseNeighbors, type NeighborCommand, type NeighborList } from "./neighborModel";

export interface NeighborClient {
  list(signal?: AbortSignal): Promise<NeighborList>;
  apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<void>;
}

async function call(command: NeighborCommand, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(command), signal,
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

export const neighborClient: NeighborClient = {
  list: async signal => parseNeighbors(await call({ operation: "list" }, signal)),
  apply: async command => { await call(command); },
};
