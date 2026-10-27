import { neighborName, type NeighborList } from "./neighborModel";

export type NeighborNodeStatus = "connected" | "paused" | "request";

export type NeighborhoodNode =
  | { kind: "neighbor"; id: string; name: string; photoURL: string | null; status: NeighborNodeStatus; x: number; y: number }
  | { kind: "request"; id: string; name: string; photoURL: string | null; status: NeighborNodeStatus; x: number; y: number }
  | { kind: "add"; id: "add"; x: number; y: number };

export const STATUS_LABEL: Record<NeighborNodeStatus, string> = {
  connected: "connected",
  paused: "paused by plan",
  request: "wants to connect",
};

/** Places everyone on one ring around you, in percent of a square stage; the add node closes the ring. */
export function neighborhoodNodes(list: NeighborList | null, canAdd: boolean): NeighborhoodNode[] {
  const people = [
    ...(list?.friends ?? []).map(friend => ({ kind: "neighbor" as const, id: friend.friendship_id, name: neighborName(friend), photoURL: friend.photoURL,
      status: list?.access === "paid" ? "connected" as const : "paused" as const })),
    ...(list?.incoming ?? []).map(request => ({ kind: "request" as const, id: request.request_id, name: neighborName(request), photoURL: request.photoURL,
      status: "request" as const })),
  ];
  const slots = people.length + (canAdd ? 1 : 0);
  const radius = slots > 8 ? 40 : 34;
  // One lone slot sits to the right of you; more slots start at the top and go clockwise.
  const start = slots === 1 ? 0 : -Math.PI / 2;
  const place = (index: number) => {
    const angle = start + (index / Math.max(slots, 1)) * Math.PI * 2;
    return { x: 50 + radius * Math.cos(angle), y: 50 + radius * Math.sin(angle) };
  };
  const nodes: NeighborhoodNode[] = people.map((person, index) => ({ ...person, ...place(index) }));
  if (canAdd) nodes.push({ kind: "add", id: "add", ...place(people.length) });
  return nodes;
}
