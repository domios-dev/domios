import { useMemo, type CSSProperties, type ReactNode } from "react";
import {
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  type UniqueIdentifier,
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
 *  its hierarchy level. dnd-kit owns each group's current prefixed-id order. */
export interface SortableGroupData {
  group: "workspace" | "tab" | "child";
  groupKey: string;
  /** Prefixed ids pinned to the front of the group (the General workspace /
   *  General tab). A drop that would not keep them leading is rejected. */
  pinnedIds?: string[];
}

interface SortableEventData extends SortableGroupData {
  sortable?: { items: UniqueIdentifier[] };
}

export const POINTER_DRAG_ACTIVATION_DISTANCE = 4;

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
    parentTabId?: string;
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
      data-dnd-disabled={disabled ? "true" : undefined}
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
    useSensor(PointerSensor, {
      activationConstraint: { distance: POINTER_DRAG_ACTIVATION_DISTANCE },
    }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const onDragEnd = useMemo(
    () => (event: DragEndEvent) => commitSidebarDrag(event, handlers),
    [handlers],
  );

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      {children}
    </DndContext>
  );
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
  const reordered = arrayMove(items, from, to);
  const pinned = activeData.pinnedIds ?? [];
  if (pinned.some((pinnedId, index) => reordered[index] !== pinnedId)) return;
  const ordered = reordered.map(rawId);
  if (activeData.group === "workspace") {
    handlers.reorderWorkspaces(ordered);
  } else if (activeData.group === "child") {
    handlers.reorderTabs({ parentTabId: activeData.groupKey, orderedTabIds: ordered });
  } else {
    handlers.reorderTabs({ workspaceId: activeData.groupKey, orderedTabIds: ordered });
  }
}

export { SortableContext, rectSortingStrategy, verticalListSortingStrategy } from "@dnd-kit/sortable";
