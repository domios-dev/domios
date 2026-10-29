import type { Neighbor, NeighborList, NeighborRequest, OutgoingNeighborRequest } from "./neighborModel";

/** What the sidebar and the graph need before the fresh list arrives: names, avatars and addresses, never messages or permissions. */
export type CachedFriend = Pick<Neighbor, "friendship_id" | "uid" | "displayName" | "email" | "photoURL" | "paused" | "my_paused"> & { gateway_tab?: string | null };
export type CachedRequest = Pick<NeighborRequest, "request_id" | "uid" | "displayName" | "email" | "photoURL">;
export type CachedInvite = Pick<OutgoingNeighborRequest, "id" | "typed_email" | "created_at">;
export interface CachedNeighbors { access: NeighborList["access"]; can_invite?: boolean; friends: CachedFriend[]; incoming: CachedRequest[]; outgoing: CachedInvite[] }

const PREFIX = "domios.neighbors.last-known:";
const MAX_FRIENDS = 200;
const key = (identity: string) => `${PREFIX}${identity}`;

export function readNeighborCache(identity: string | null): CachedNeighbors | null {
  if (!identity) return null;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key(identity)) ?? "null");
    if (typeof value !== "object" || value === null) return null;
    const { access, friends, incoming, outgoing, can_invite } = value as { access?: unknown; friends?: unknown; incoming?: unknown; outgoing?: unknown; can_invite?: unknown };
    if ((access !== "paid" && access !== "locked") || !Array.isArray(friends)) return null;
    const valid = friends.filter((friend): friend is CachedFriend => typeof friend === "object" && friend !== null
      && typeof friend.friendship_id === "string" && typeof friend.uid === "string"
      && (friend.displayName === null || typeof friend.displayName === "string")
      && (friend.photoURL === null || typeof friend.photoURL === "string")
      && (friend.email === undefined || friend.email === null || typeof friend.email === "string"));
    const text = (field: unknown) => field === null || typeof field === "string";
    const requests = (Array.isArray(incoming) ? incoming : []).filter((request): request is CachedRequest => typeof request === "object" && request !== null
      && typeof request.request_id === "string" && typeof request.uid === "string" && text(request.displayName) && text(request.photoURL)
      && (request.email === undefined || text(request.email)));
    const invites = (Array.isArray(outgoing) ? outgoing : []).filter((invite): invite is CachedInvite => typeof invite === "object" && invite !== null
      && typeof invite.id === "string" && typeof invite.typed_email === "string" && typeof invite.created_at === "number");
    return { access, friends: valid.slice(0, MAX_FRIENDS), incoming: requests.slice(0, MAX_FRIENDS), outgoing: invites.slice(0, MAX_FRIENDS),
      ...(typeof can_invite === "boolean" ? { can_invite } : {}) };
  } catch { return null; }
}

export function writeNeighborCache(identity: string, list: NeighborList) {
  const friends = list.friends.slice(0, MAX_FRIENDS).map(({ friendship_id, uid, displayName, email, photoURL, gateway_tab }) =>
    ({ friendship_id, uid, displayName, photoURL, ...(email === undefined ? {} : { email }), ...(gateway_tab === undefined ? {} : { gateway_tab }) }));
  const incoming = list.incoming.slice(0, MAX_FRIENDS).map(({ request_id, uid, displayName, email, photoURL }) => ({ request_id, uid, displayName, photoURL, ...(email === undefined ? {} : { email }) }));
  const outgoing = list.outgoing.slice(0, MAX_FRIENDS).map(({ id, typed_email, created_at }) => ({ id, typed_email, created_at }));
  const cached: CachedNeighbors = { access: list.access, friends, incoming, outgoing, ...(list.can_invite === undefined ? {} : { can_invite: list.can_invite }) };
  try { localStorage.setItem(key(identity), JSON.stringify(cached)); }
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
