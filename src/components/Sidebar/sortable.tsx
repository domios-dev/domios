import { useMemo, type CSSProperties, type ReactNode } from "react";
import {
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

/** Each sortable item carries the group it belongs to. A reorder is applied
 *  only when active + over share the same group, so an item can never cross
 *  its hierarchy level. `items` is the group's current prefixed-id order. */
export interface SortableGroupData {
  group: "workspace" | "tab" | "child";
  groupKey: string;
  items: string[];
  /** Prefixed ids pinned to the front of the group (the General workspace /
   *  General tab). A drop that would not keep them leading is rejected. */
  pinnedIds?: string[];
}

export const WS_PREFIX = "ws:";
export const TAB_PREFIX = "tab:";

/** Strip the type prefix off a sortable id, yielding the raw backend id. */
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
    parentPaneId?: string;
    orderedTabIds: string[];
  }) => void;
}

/** A single draggable/sortable row. Drag listeners ride the wrapper, so the
 *  whole row is the handle; the pointer activation distance keeps a plain
 *  click (tab/workspace activation) from starting a drag. */
export function SortableRow({
  id,
  data,
  disabled,
  layoutStyle,
  children,
}: {
  id: string;
  data: SortableGroupData;
  disabled?: boolean;
  layoutStyle?: CSSProperties;
  children: ReactNode;
}) {
  const { setNodeRef, transform, transition, attributes, listeners, isDragging } =
    useSortable({ id, data, disabled });
  return (
    <div
      ref={setNodeRef}
      data-dnd-group={`${data.group}:${data.groupKey}`}
      data-dnd-dragging={isDragging ? "true" : undefined}
      style={{
        ...layoutStyle,
        transform: CSS.Translate.toString(transform),
        transition,
        opacity: isDragging ? 0.5 : undefined,
        position: "relative",
        zIndex: isDragging ? 10 : undefined,
      }}
      {...attributes}
      {...listeners}
    >
      {children}
    </div>
  );
}

/** Wraps the sidebar's sortable region in one DndContext. A drag is committed
 *  only within its own group (see SortableGroupData). */
export function SidebarDnd({
  handlers,
  children,
}: {
  handlers: ReorderHandlers;
  children: ReactNode;
}) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const onDragEnd = useMemo(
    () => (e: DragEndEvent) => {
      const { active, over } = e;
      if (!over || active.id === over.id) return;
      const ad = active.data.current as SortableGroupData | undefined;
      const od = over.data.current as SortableGroupData | undefined;
      if (!ad || !od) return;
      // Within-group only — dropping onto another group is a no-op (snap back).
      if (ad.group !== od.group || ad.groupKey !== od.groupKey) return;
      const from = ad.items.indexOf(String(active.id));
      const to = ad.items.indexOf(String(over.id));
      if (from < 0 || to < 0) return;
      const reordered = arrayMove(ad.items, from, to);
      // the group in their original order — nothing reorders above General.
      const pinned = ad.pinnedIds ?? [];
      if (pinned.some((p, i) => reordered[i] !== p)) return;
      const ordered = reordered.map(rawId);
      if (ad.group === "workspace") {
        handlers.reorderWorkspaces(ordered);
      } else if (ad.group === "child") {
        handlers.reorderTabs({ parentPaneId: ad.groupKey, orderedTabIds: ordered });
      } else {
        handlers.reorderTabs({ workspaceId: ad.groupKey, orderedTabIds: ordered });
      }
    },
    [handlers],
  );

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      {children}
    </DndContext>
  );
}

export { SortableContext, rectSortingStrategy, verticalListSortingStrategy } from "@dnd-kit/sortable";
