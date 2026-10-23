import { useMemo, useRef, type ComponentProps, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import {
  type CollisionDetection,
  DndContext,
  type Modifier,
  type DragEndEvent,
  type UniqueIdentifier,
  closestCenter,
  useDndContext,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext as DndSortableContext,
  useSortable,
} from "@dnd-kit/sortable";
import { sidebarDropEdge, sidebarDropTarget } from "./sidebarDropGeometry";
import { SidebarDragPreview } from "./SidebarDragPreview";
import { sortableSensorConfiguration } from "./sortableSensors";
export { POINTER_DRAG_ACTIVATION_DISTANCE } from "./sortableSensors";

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

export const WS_PREFIX = "ws:";
export const TAB_PREFIX = "tab:";
export const TEAM_PREFIX = "team:";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.closest('input, textarea, select, [contenteditable], [role="textbox"]') !== null;
}

/** Cocoa WebKit focuses the sortable wrapper around a pressed button and scrolls it into view.
 *  Attach above every SortableRow: dnd-kit ignores a press whose default is already prevented. */
export function keepPressFocusInPlace(event: ReactMouseEvent<HTMLElement>): void {
  if (event.button !== 0 || event.defaultPrevented || isEditableTarget(event.target)) return;
  if (event.target instanceof Element && event.target.closest("[data-dnd-group]")) event.preventDefault();
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
  const { setNodeRef, attributes, listeners, isDragging, items } =
    useSortable({ id, data, disabled });
  const { active, over } = useDndContext();
  const edge = active && over?.id === id
    ? sidebarDropEdge(items.map(String), String(active.id), id) : null;
  return (
    <div
      ref={setNodeRef}
      data-dnd-id={id}
      data-dnd-group={`${data.group}:${data.groupKey}`}
      data-dnd-dragging={isDragging ? "true" : undefined}
      data-dnd-disabled={disabled ? "true" : undefined}
      style={{
        ...layoutStyle,
        touchAction: "auto",
        position: "relative",
        opacity: isDragging ? 0.35 : undefined,
      }}
      {...attributes}
      aria-disabled={undefined}
      {...listeners}
      onMouseDown={(event) => {
        if (isEditableTarget(event.target)) return;
        listeners?.onMouseDown?.(event);
      }}
      onTouchStart={(event) => {
        if (isEditableTarget(event.target)) return;
        listeners?.onTouchStart?.(event);
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        listeners?.onKeyDown?.(event);
      }}
    >
      {children}
      {edge && <div aria-hidden="true" data-dnd-indicator={edge} style={{
        position: "absolute", left: 0, right: 0, [edge]: 0, height: 2,
        background: "var(--ct-accent)", pointerEvents: "none", zIndex: 1,
      }} />}
    </div>
  );
}

// Sensor options are compared by identity; fresh objects would hand every sortable row a new context each render.
const MOUSE_SENSOR = sortableSensorConfiguration("mouse");
const TOUCH_SENSOR = sortableSensorConfiguration("touch");

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
    useSensor(MOUSE_SENSOR.sensor, MOUSE_SENSOR.options),
    useSensor(TOUCH_SENSOR.sensor, TOUCH_SENSOR.options),
    useSensor(MOUSE_SENSOR.keyboard.sensor, MOUSE_SENSOR.keyboard.options),
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
      autoScroll={false}
      onDragEnd={onDragEnd}
    >
      {children}
      <SidebarDragPreview />
    </DndContext>
  );
}

/** Tab rows travel only vertically; the workspace card grid keeps both axes. */
export const lockListRowsToVerticalAxis: Modifier = ({ active, transform }) => {
  const group = (active?.data.current as SortableGroupData | undefined)?.group;
  return group && group !== "workspace" ? { ...transform, x: 0 } : transform;
};

function sameGroup(a: SortableGroupData | undefined, b: SortableGroupData | undefined): boolean {
  return Boolean(a && b && a.group === b.group && a.groupKey === b.groupKey);
}

/**
 * Only movable rows of the dragged row's own group can be targets: nested groups never steal the drop,
 * and a pinned row is never chosen only to have the reorder refused and the row fly back.
 */
export const sameGroupCollision: CollisionDetection = (args) => {
  const activeData = args.active.data.current as SortableGroupData | undefined;
  const containers = args.droppableContainers.filter((container) =>
    !container.disabled && sameGroup(activeData, container.data.current as SortableGroupData | undefined));
  if (activeData?.group !== "workspace" && args.pointerCoordinates) {
    const rows = containers.flatMap(container => {
      const rect = args.droppableRects.get(container.id);
      return rect ? [{ id: String(container.id), top: rect.top, bottom: rect.bottom }] : [];
    }).sort((a, b) => a.top - b.top);
    const target = sidebarDropTarget(rows, String(args.active.id),
      args.pointerCoordinates?.y ?? args.collisionRect.top + args.collisionRect.height / 2);
    const container = containers.find(candidate => String(candidate.id) === target);
    return container ? [{ id: container.id, data: { droppableContainer: container, value: 1 } }] : [];
  }
  return closestCenter({
    ...args,
    droppableContainers: containers,
  });
};

/** A dragged row cannot travel above the pinned rows that head its own list. */
export const keepBelowPinnedRows: Modifier = ({ active, activeNodeRect, transform }) => {
  const data = active?.data.current as SortableGroupData | undefined;
  if (!data || !activeNodeRect || data.group === "workspace") return transform;
  const pinned = document.querySelectorAll(`[data-dnd-group="${globalThis.CSS.escape(`${data.group}:${data.groupKey}`)}"][data-dnd-disabled="true"]`);
  let floor = -Infinity;
  for (const node of pinned) floor = Math.max(floor, node.getBoundingClientRect().bottom);
  if (floor === -Infinity) return transform;
  return { ...transform, y: Math.max(transform.y, floor - activeNodeRect.top) };
};

const LIST_AXIS_MODIFIERS = [lockListRowsToVerticalAxis, keepBelowPinnedRows];

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

/** dnd-kit publishes a new context for every new items array, waking each sortable row; equal ids keep the array. */
export function SortableContext({ items, ...props }: ComponentProps<typeof DndSortableContext>) {
  const stable = useRef(items);
  if (stable.current.length !== items.length
    || stable.current.some((item, index) => sortableId(item) !== sortableId(items[index]))) stable.current = items;
  return <DndSortableContext items={stable.current} {...props} />;
}

function sortableId(item: UniqueIdentifier | { id: UniqueIdentifier }): UniqueIdentifier {
  return typeof item === "object" ? item.id : item;
}

export { rectSortingStrategy, verticalListSortingStrategy } from "@dnd-kit/sortable";
