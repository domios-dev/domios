export const COMPOSER_DRAFT_PREFIX = "codeterm:composer-draft:";

// Composer drafts persist under `${COMPOSER_DRAFT_PREFIX}${channel}:${paneId}`.
// Every channel keys on the pane id, so a draft is orphaned the moment its pane
// leaves the live mux. Drafts are only ever cleared on a successful send, never
// on pane close — without this reclaim a reused/colliding pane id would inherit
// a dead pane's unsent text.
const PANE_SCOPED_CHANNELS = ["pane-session-chat", "chat-view"] as const;

type StorageLike = Pick<Storage, "length" | "key" | "getItem" | "removeItem">;

/** Pane id owning a composer-draft key, or null if the key is not a known
 *  pane-scoped draft. Pane ids are colon-free short ids, so the channel is the
 *  segment before the first colon and the pane id is the remainder. */
export function draftKeyPaneId(key: string): string | null {
  if (!key.startsWith(COMPOSER_DRAFT_PREFIX)) return null;
  const scope = key.slice(COMPOSER_DRAFT_PREFIX.length);
  const colon = scope.indexOf(":");
  if (colon < 0) return null;
  const channel = scope.slice(0, colon);
  if (!(PANE_SCOPED_CHANNELS as readonly string[]).includes(channel)) return null;
  return scope.slice(colon + 1) || null;
}

/** Remove every composer draft whose pane id is no longer live. Returns the
 *  removed keys. Live panes keep their drafts untouched. Best-effort: skip when
 *  the live set is empty (pre-hydration) so a transient empty snapshot can't
 *  wipe a live pane's unsent draft. */
export function pruneOrphanComposerDrafts(
  storage: StorageLike,
  livePaneIds: ReadonlySet<string>,
): string[] {
  if (livePaneIds.size === 0) return [];
  const candidates: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const key = storage.key(i);
    if (key && key.startsWith(COMPOSER_DRAFT_PREFIX)) candidates.push(key);
  }
  const removed: string[] = [];
  for (const key of candidates) {
    const paneId = draftKeyPaneId(key);
    if (paneId && !livePaneIds.has(paneId)) {
      storage.removeItem(key);
      removed.push(key);
    }
  }
  return removed;
}
