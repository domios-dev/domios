import { createContext, useContext, type ReactNode } from "react";
import type { Neighbor } from "../Neighbors/neighborModel";
import { useNeighbors } from "../Neighbors/useNeighbors";
import { useAccount } from "../Settings/useAccount";
import type { MemAuthor } from "./types";

export type MemoryFriend = Pick<Neighbor, "uid" | "friendship_id" | "displayName">;
export const MemoryFriendsContext = createContext<readonly MemoryFriend[]>([]);

function friendName(author: MemAuthor, friends: readonly MemoryFriend[]): string | null {
  const friend = friends.find(friend => author.friendship_id
    ? friend.friendship_id === author.friendship_id && (!author.account_uid || friend.uid === author.account_uid)
    : friend.uid === author.account_uid);
  return friend?.displayName?.trim() || null;
}

export function memoryProvenance(author: MemAuthor | null | undefined, friends: readonly MemoryFriend[]): string | null {
  if (author?.kind !== "neighbor") return null;
  const name = friendName(author, friends);
  return name ? `From ${name} · neighbor` : "From a neighbor";
}

export function MemoryFriendsProvider({ children }: { children: ReactNode }) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  const { list } = useNeighbors(identity);
  return <MemoryFriendsContext.Provider value={list?.friends ?? []}>{children}</MemoryFriendsContext.Provider>;
}

export function MemoryProvenance({ author, createdAt, compact = false }: { author?: MemAuthor | null; createdAt?: string | null; compact?: boolean }) {
  const friends = useContext(MemoryFriendsContext);
  const label = memoryProvenance(author, friends);
  if (!label || !author) return null;
  const name = friendName(author, friends) ?? "a neighbor";
  const initials = name === "a neighbor" ? "?" : name.split(/\s+/).map(part => Array.from(part)[0]).slice(0, 2).join("").toUpperCase();
  const date = createdAt ? new Date(createdAt) : null;
  const savedAt = date && Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(date) : null;
  const agent = author?.label?.trim();
  const saved = [agent ? `Saved by their agent ${agent}` : savedAt ? "Saved" : null, savedAt].filter(Boolean).join(" · ");
  return <div data-testid="memory-provenance" aria-label={label} className="text-xs">
    <div className="flex items-center gap-1.5 text-[var(--ct-fg)]">
      <span aria-hidden="true" className="shrink-0 flex items-center justify-center text-[var(--ct-fg)]" style={{ width: 16, height: 16, borderRadius: "50%", background: "var(--ct-surface)", border: "1px solid var(--ct-border-default)", fontSize: 10, fontWeight: 600 }}>{initials}</span>
      <span>From <strong className="font-semibold">{name}</strong></span>
      <span className="text-[11px] px-1.5 rounded border text-[var(--ct-accent)]" style={{ borderColor: "color-mix(in srgb, var(--ct-accent) 35%, transparent)" }}>neighbor</span>
    </div>
    {!compact && saved && <div className="mt-1 pl-[22px] text-[var(--ct-muted-fg)]">{saved}</div>}
  </div>;
}
