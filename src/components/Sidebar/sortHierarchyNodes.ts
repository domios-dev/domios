import type { Pane, PaneStatus } from "../../lib/types";

export const HIERARCHY_SORT_STABILITY_MS = 2000;

export interface HierarchyNodeBase {
  pane: Pane;
  status: PaneStatus | null;
}

export interface HierarchySortState {
  order: string[];
  committedStatus: Record<string, PaneStatus | null>;
  pendingStatus: Record<string, PaneStatus | null>;
  pendingSince: Record<string, number>;
}

export function statusSortRank(status: PaneStatus | null): number {
  switch (status) {
    case "Working":
      return 0;
    case "Waiting":
      return 1;
    case "Idle":
      return 2;
    case "Dead":
      return 3;
    default:
      return 2;
  }
}

function currentStatus(
  node: HierarchyNodeBase,
  statuses: Readonly<Record<string, PaneStatus>>,
): PaneStatus | null {
  return statuses[node.pane.id] ?? node.status ?? null;
}

function compareByCommitted<T extends HierarchyNodeBase>(
  a: T,
  b: T,
  committed: Readonly<Record<string, PaneStatus | null>>,
  tiebreak: (id: string) => number,
): number {
  const ra = statusSortRank(committed[a.pane.id] ?? null);
  const rb = statusSortRank(committed[b.pane.id] ?? null);
  if (ra !== rb) return ra - rb;
  const ta = tiebreak(a.pane.id);
  const tb = tiebreak(b.pane.id);
  if (ta !== tb) return ta - tb;
  return a.pane.id < b.pane.id ? -1 : a.pane.id > b.pane.id ? 1 : 0;
}

export function sortHierarchyNodes<T extends HierarchyNodeBase>(
  nodes: readonly T[],
  statuses: Readonly<Record<string, PaneStatus>>,
  prev: HierarchySortState | null,
  now: number,
): { nodes: T[]; state: HierarchySortState } {
  const ids = nodes.map((n) => n.pane.id);
  const committedStatus: Record<string, PaneStatus | null> = { ...(prev?.committedStatus ?? {}) };
  const pendingStatus: Record<string, PaneStatus | null> = { ...(prev?.pendingStatus ?? {}) };
  const pendingSince: Record<string, number> = { ...(prev?.pendingSince ?? {}) };

  for (const node of nodes) {
    const id = node.pane.id;
    const live = currentStatus(node, statuses);
    const prevPending = pendingStatus[id];
    if (prevPending === undefined) {
      pendingStatus[id] = live;
      pendingSince[id] = now;
      committedStatus[id] = live;
      continue;
    }
    if (live !== prevPending) {
      pendingStatus[id] = live;
      pendingSince[id] = now;
    }
    const since = pendingSince[id] ?? now;
    if (now - since >= HIERARCHY_SORT_STABILITY_MS) {
      committedStatus[id] = pendingStatus[id] ?? live;
    }
  }

  const prevOrder = prev?.order ?? ids;
  const tiebreak = (id: string) => {
    const idx = prevOrder.indexOf(id);
    return idx === -1 ? ids.indexOf(id) : idx;
  };

  const sorted = [...nodes].sort((a, b) =>
    compareByCommitted(a, b, committedStatus, tiebreak),
  );
  return {
    nodes: sorted,
    state: {
      order: sorted.map((n) => n.pane.id),
      committedStatus,
      pendingStatus,
      pendingSince,
    },
  };
}
