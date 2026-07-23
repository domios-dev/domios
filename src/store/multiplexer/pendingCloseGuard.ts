import type { MultiplexerState } from "../../lib/types";

/** Pane ids optimistically removed by a local close, awaiting backend
 *  confirmation. A tombstone is cleared ONLY by an authoritative transition —
 *  a snapshot that omits the pane, an explicit close failure, or a deliberate
 *  reopen — never by elapsed time, since a time-based clear would re-open the
 *  same stale-snapshot resurrection window. */
export type CloseGuards = Set<string>;

// Masks optimistically-closed panes out of a backend snapshot so a snapshot
// whose read raced ahead of the close cannot resurrect the just-removed tab.
// Mutates `closed`: drops a tombstone once the snapshot confirms removal (the
// pane is absent and no tab references it). Returns `s` unchanged (identity)
// when nothing is masked.
export function applyPendingCloseGuard(
  s: MultiplexerState,
  closed: CloseGuards,
): MultiplexerState {
  if (closed.size === 0) return s;

  const toMask = new Set<string>();
  for (const paneId of closed) {
    const present =
      Boolean(s.panes[paneId]) ||
      s.workspaces.some((ws) => ws.tabs.some((t) => t.pane_id === paneId));
    if (!present) {
      closed.delete(paneId);
      continue;
    }
    toMask.add(paneId);
  }
  if (toMask.size === 0) return s;

  const panes = { ...s.panes };
  for (const id of toMask) delete panes[id];
  const workspaces = s.workspaces.map((ws) =>
    ws.tabs.some((t) => toMask.has(t.pane_id))
      ? { ...ws, tabs: ws.tabs.filter((t) => !toMask.has(t.pane_id)) }
      : ws,
  );
  return { ...s, panes, workspaces };
}
