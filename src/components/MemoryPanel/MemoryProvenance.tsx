import { createContext, useContext, type ReactNode } from "react";
import type { Neighbor } from "../Neighbors/neighborModel";
import { useNeighbors } from "../Neighbors/useNeighbors";
import { useAccount } from "../Settings/useAccount";
import type { MemAuthor } from "./types";
import { Chip } from "../shared/Chip";
import { colorForKind } from "./nodeKind";

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
  return <div data-testid="memory-provenance" aria-label={label} className="min-w-0 text-xs text-[var(--ct-muted-fg)]">
    <div className="flex min-w-0 items-center gap-1.5">
      <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center rounded-full bg-[var(--ct-bg-hover)] text-[10px] font-semibold text-[var(--ct-fg)]">{initials}</span>
      <span className="min-w-0 truncate">From <strong className="font-semibold text-[var(--ct-fg)]">{name}</strong> &middot; neighbor</span>
    </div>
    {!compact && saved && <div className="mt-1 pl-[22px] text-[var(--ct-muted-fg)]">{saved}</div>}
  </div>;
}

export function MemoryKindChip({ kind }: { kind: string }) {
  const color = colorForKind(kind);
  return <Chip className="!h-[22px] !rounded-md !border-0 !px-2 !text-xs !font-semibold" style={{ background: `color-mix(in srgb, ${color} 14%, transparent)`, color }}>{kind}</Chip>;
}

export function MemoryImportance({ value }: { value: number }) {
  return (
    <span className="inline-flex items-center gap-1.5" title={`Importance: ${value} of 5`}>
      Importance <span className="sr-only">{value} of 5</span>
      <span aria-hidden="true" className="inline-flex gap-[3px]">
        {[1, 2, 3, 4, 5].map(level => (
          <span key={level} className="size-[7px] rounded-full bg-[var(--ct-fg)]/18" style={{ backgroundImage: `linear-gradient(to right, var(--ct-accent) ${Math.max(0, Math.min(1, value - level + 1)) * 100}%, transparent 0)` }} />
        ))}
      </span>
    </span>
  );
}
