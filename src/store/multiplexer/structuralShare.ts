import type { ExtendedState } from "./tree";
import { sameJson, shareRecord } from "../../lib/sameJson";

/** Keeps previous items matched by `keyOf` whose content did not change: the previous array when none did, the incoming one when all did. */
export function shareList<T>(previous: readonly T[], next: T[], keyOf: (item: T) => string): T[] {
  if (previous === next) return next;
  const byKey = new Map(previous.map((item) => [keyOf(item), item]));
  let changed = previous.length !== next.length;
  let reused = 0;
  const shared = next.map((item, index) => {
    const before = byKey.get(keyOf(item));
    if (before !== undefined && sameJson(before, item)) {
      if (previous[index] !== before) changed = true;
      reused++;
      return before;
    }
    changed = true;
    return item;
  });
  if (!changed) return previous as T[];
  return reused === 0 ? next : shared;
}

/** Gives an accepted snapshot the previous identities of everything it left unchanged. */
export function shareSnapshotState(previous: ExtendedState, next: ExtendedState): ExtendedState {
  const shared: ExtendedState = {
    ...next,
    workspaces: shareList(previous.workspaces, next.workspaces, (workspace) => workspace.id),
    tabs: shareRecord(previous.tabs, next.tabs) as ExtendedState["tabs"],
    teams: shareList(previous.teams, next.teams, (team) => team.id),
    activeTabIds: shareRecord(previous.activeTabIds, next.activeTabIds) as ExtendedState["activeTabIds"],
    tabStatuses: shareRecord(previous.tabStatuses, next.tabStatuses) as ExtendedState["tabStatuses"],
    recently_closed: sameJson(previous.recently_closed, next.recently_closed) ? previous.recently_closed : next.recently_closed,
    closed_structures: sameJson(previous.closed_structures, next.closed_structures) ? previous.closed_structures : next.closed_structures,
    pendingCloses: sameJson(previous.pendingCloses, next.pendingCloses) ? previous.pendingCloses : next.pendingCloses,
  };
  for (const key of Object.keys(shared) as Array<keyof ExtendedState>) {
    if (key !== "generation" && shared[key] !== previous[key]) return shared;
  }
  return previous.generation === shared.generation ? previous : { ...previous, generation: shared.generation };
}
