export const COMPOSER_DRAFT_PREFIX = "codeterm:composer-draft:";

// Composer drafts persist under `${COMPOSER_DRAFT_PREFIX}${channel}:${tabId}`.
// Every channel keys on the tab id, so a draft is orphaned the moment its tab
// leaves the live mux. Drafts are only ever cleared on a successful send, never
// on tab close — without this reclaim a reused/colliding tab id would inherit
// a dead tab's unsent text.
const TAB_SCOPED_CHANNELS = ["tab-session-chat", "chat-view"] as const;

type StorageLike = Pick<Storage, "length" | "key" | "getItem" | "removeItem">;

/** Tab id owning a composer-draft key, or null if the key is not a known
 *  tab-scoped draft. Tab ids are colon-free short ids, so the channel is the
 *  segment before the first colon and the tab id is the remainder. */
export function draftKeyTabId(key: string): string | null {
  if (!key.startsWith(COMPOSER_DRAFT_PREFIX)) return null;
  const scope = key.slice(COMPOSER_DRAFT_PREFIX.length);
  const colon = scope.indexOf(":");
  if (colon < 0) return null;
  const channel = scope.slice(0, colon);
  if (!(TAB_SCOPED_CHANNELS as readonly string[]).includes(channel)) return null;
  return scope.slice(colon + 1) || null;
}

/** Remove every composer draft whose tab id is no longer live. Returns the
 *  removed keys. Live tabs keep their drafts untouched. Best-effort: skip when
 *  the live set is empty (pre-hydration) so a transient empty snapshot can't
 *  wipe a live tab's unsent draft. */
export function pruneOrphanComposerDrafts(
  storage: StorageLike,
  liveTabIds: ReadonlySet<string>,
): string[] {
  if (liveTabIds.size === 0) return [];
  const candidates: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const key = storage.key(i);
    if (key && key.startsWith(COMPOSER_DRAFT_PREFIX)) candidates.push(key);
  }
  const removed: string[] = [];
  for (const key of candidates) {
    const tabId = draftKeyTabId(key);
    if (tabId && !liveTabIds.has(tabId)) {
      storage.removeItem(key);
      removed.push(key);
    }
  }
  return removed;
}
