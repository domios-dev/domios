import { commitSidebarDrag, type ReorderHandlers, type SortableGroupData } from "./sidebarReorderModel";
export { commitSidebarDrag, rawId, sortByOrder, WS_PREFIX, TAB_PREFIX, TEAM_PREFIX, type ReorderHandlers, type SortableGroupData } from "./sidebarReorderModel";
import { useMemo, useRef, type ComponentProps, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import {
  type CollisionDetection,
  type DragEndEvent,
  DndContext,
  type Modifier,
  type UniqueIdentifier,
  closestCenter,
  useDndContext,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext as DndSortableContext,
  useSortable,
} from "@dnd-kit/sortable";
import { sidebarDropEdge, sidebarDropTarget } from "./sidebarDropGeometry";
import { SidebarDragPreview } from "./SidebarDragPreview";
import { sortableSensorConfiguration } from "./sortableSensors";
export { POINTER_DRAG_ACTIVATION_DISTANCE } from "./sortableSensors";

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
