export const POINTER_DRAG_ACTIVATION_DISTANCE = 10;
import type { DragEndEvent, UniqueIdentifier } from "@dnd-kit/core";

export interface SortableGroupData {
  group: "workspace" | "tab" | "team" | "child";
  groupKey: string;
  /** Prefixed ids pinned to the front of the group (the General workspace /
   *  General tab). A drop that would not keep them leading is rejected. */
  pinnedIds?: string[];
  /** Expands a reordered visible subset into the full order the backend expects. */
  fullOrder?: (orderedRawIds: string[]) => string[];
  /** Expands the group's new order into the full display order of its workspace. */
  toWorkspaceOrder?: (orderedRawIds: string[]) => { workspaceId: string; orderedTabIds: string[] };
}

interface SortableEventData extends SortableGroupData {
  sortable?: { items: UniqueIdentifier[] };
}

export const WS_PREFIX = "ws:";
export const TAB_PREFIX = "tab:";
export const TEAM_PREFIX = "team:";

export function rawId(prefixed: string): string {
  const idx = prefixed.indexOf(":");
  return idx === -1 ? prefixed : prefixed.slice(idx + 1);
}

/** Stable copy sorted by `sort_order` ascending (missing → 0). */
export function sortByOrder<T extends { sort_order?: number }>(items: T[]): T[] {
  return [...items].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export interface ReorderHandlers {
  reorderWorkspaces: (orderedWorkspaceIds: string[]) => void;
  reorderTabs: (args: {
    workspaceId?: string;
    parentTabId?: string;
    orderedTabIds: string[];
  }) => void;
  reorderDisplay: (args: { workspaceId: string; orderedTabIds: string[] }) => void;
}


export function commitSidebarDrag(event: DragEndEvent, handlers: ReorderHandlers): void {
  const { active, over } = event;
  if (!over || active.id === over.id) return;
  const activeData = active.data.current as SortableEventData | undefined;
  const overData = over.data.current as SortableEventData | undefined;
  if (!activeData || !overData) return;
  if (
    activeData.group !== overData.group
    || activeData.groupKey !== overData.groupKey
  ) return;
  const items = activeData.sortable?.items.map(String) ?? [];
  const from = items.indexOf(String(active.id));
  const to = items.indexOf(String(over.id));
  if (from < 0 || to < 0) return;
  const reordered = [...items];
  reordered.splice(to, 0, ...reordered.splice(from, 1));
  const pinned = activeData.pinnedIds ?? [];
  if (pinned.some((pinnedId, index) => reordered[index] !== pinnedId)) return;
  const ordered = reordered.map(rawId);
  if (activeData.toWorkspaceOrder) {
    handlers.reorderDisplay(activeData.toWorkspaceOrder(ordered));
    return;
  }
  if (activeData.group === "workspace") {
    handlers.reorderWorkspaces(activeData.fullOrder ? activeData.fullOrder(ordered) : ordered);
  } else if (activeData.group === "child") {
    handlers.reorderTabs({ parentTabId: activeData.groupKey, orderedTabIds: ordered });
  } else {
    handlers.reorderTabs({ workspaceId: activeData.groupKey, orderedTabIds: ordered });
  }
}

