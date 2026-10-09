import { useMemo, type CSSProperties, type ReactNode } from "react";
import {
  type CollisionDetection,
  DndContext,
  type Modifier,
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

export const POINTER_DRAG_ACTIVATION_DISTANCE = 4;

export const WS_PREFIX = "ws:";
export const TAB_PREFIX = "tab:";
export const TEAM_PREFIX = "team:";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.matches("input, textarea, select, [contenteditable]")
    || target.closest("input, textarea, select, [contenteditable]") !== null;
}

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
  reorderDisplay: (args: { workspaceId: string; orderedTabIds: string[] }) => void;
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
      onKeyDown={(event) => {
        if (isEditableTarget(event.target)) return;
        listeners?.onKeyDown?.(event);
      }}
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
    <DndContext
      sensors={sensors}
      collisionDetection={sameGroupCollision}
      modifiers={LIST_AXIS_MODIFIERS}
      autoScroll={VERTICAL_AUTO_SCROLL}
      onDragEnd={onDragEnd}
    >
      {children}
    </DndContext>
  );
}

/** Tab rows travel only vertically; the workspace card grid keeps both axes. */
export const lockListRowsToVerticalAxis: Modifier = ({ active, transform }) => {
  const group = (active?.data.current as SortableGroupData | undefined)?.group;
  return group && group !== "workspace" ? { ...transform, x: 0 } : transform;
};

const LIST_AXIS_MODIFIERS = [lockListRowsToVerticalAxis];
const VERTICAL_AUTO_SCROLL = { threshold: { x: 0, y: 0.2 } };

function sameGroup(a: SortableGroupData | undefined, b: SortableGroupData | undefined): boolean {
  return Boolean(a && b && a.group === b.group && a.groupKey === b.groupKey);
}

/** Only rows of the dragged row's own group can be targets, so nested groups never steal the drop. */
export const sameGroupCollision: CollisionDetection = (args) => {
  const activeData = args.active.data.current as SortableGroupData | undefined;
  return closestCenter({
    ...args,
    droppableContainers: args.droppableContainers.filter((container) =>
      sameGroup(activeData, container.data.current as SortableGroupData | undefined)),
  });
};

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

export { SortableContext, rectSortingStrategy, verticalListSortingStrategy } from "@dnd-kit/sortable";
