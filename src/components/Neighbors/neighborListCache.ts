import type { Neighbor, NeighborList } from "./neighborModel";

/** What the sidebar needs to draw a friend before the fresh list arrives: never messages or permissions. */
export type CachedFriend = Pick<Neighbor, "friendship_id" | "uid" | "displayName" | "photoURL"> & { gateway_tab?: string | null };
export interface CachedNeighbors { access: NeighborList["access"]; friends: CachedFriend[] }

const PREFIX = "domios.neighbors.last-known:";
const MAX_FRIENDS = 200;
const key = (identity: string) => `${PREFIX}${identity}`;

export function readNeighborCache(identity: string | null): CachedNeighbors | null {
  if (!identity) return null;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key(identity)) ?? "null");
    if (typeof value !== "object" || value === null) return null;
    const { access, friends } = value as { access?: unknown; friends?: unknown };
    if ((access !== "paid" && access !== "locked") || !Array.isArray(friends)) return null;
    const valid = friends.filter((friend): friend is CachedFriend => typeof friend === "object" && friend !== null
      && typeof friend.friendship_id === "string" && typeof friend.uid === "string"
      && (friend.displayName === null || typeof friend.displayName === "string")
      && (friend.photoURL === null || typeof friend.photoURL === "string"));
    return { access, friends: valid.slice(0, MAX_FRIENDS) };
  } catch { return null; }
}

export function writeNeighborCache(identity: string, list: NeighborList) {
  const friends = list.friends.slice(0, MAX_FRIENDS).map(({ friendship_id, uid, displayName, photoURL, gateway_tab }) =>
    ({ friendship_id, uid, displayName, photoURL, ...(gateway_tab === undefined ? {} : { gateway_tab }) }));
  try { localStorage.setItem(key(identity), JSON.stringify({ access: list.access, friends })); }
  catch { /* The cache only speeds up the first paint; the fresh list still arrives. */ }
}

/** Forget every account's last known friends except `keep`; signing out keeps none. */
export function clearNeighborCaches(keep: string | null = null) {
  try {
    const stale: string[] = [];
    for (let index = 0; index < localStorage.length; index++) {
      const name = localStorage.key(index);
      if (name?.startsWith(PREFIX) && name !== (keep ? key(keep) : null)) stale.push(name);
    }
    stale.forEach(name => localStorage.removeItem(name));
  } catch { /* Storage can be unavailable in a private viewer. */ }
}
