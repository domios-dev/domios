import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { patchPlanLayout } from "./plansApi";
import type { LaidOutNode, WorkspaceLayout } from "./layout";
import type { ViewportTransform } from "./useCanvasViewport";

export function isDraggableNodeKind(kind: string): boolean {
  return kind !== "plan";
}

export interface NodeDragOverride {
  nodeId: string;
  x: number;
  y: number;
}

interface ActiveDrag {
  nodeId: string;
  planId: string;
  frameX: number;
  frameY: number;
  grabOffsetX: number;
  grabOffsetY: number;
}

export interface UseCanvasNodeDragOptions {
  api?: ApiConfig;
  containerRef: React.RefObject<HTMLDivElement | null>;
  transform: ViewportTransform;
  layout: WorkspaceLayout;
}

export interface UseCanvasNodeDragResult {
  dragOverride: NodeDragOverride | null;
  draggingNodeId: string | null;
  onNodeMouseDown: (e: React.MouseEvent, node: LaidOutNode) => void;
}

function clientToWorld(
  clientX: number,
  clientY: number,
  container: HTMLDivElement | null,
  transform: ViewportTransform,
): { x: number; y: number } | null {
  const rect = container?.getBoundingClientRect();
  if (!rect) return null;
  return {
    x: (clientX - rect.left - transform.x) / transform.scale,
    y: (clientY - rect.top - transform.y) / transform.scale,
  };
}

export function useCanvasNodeDrag({
  api,
  containerRef,
  transform,
  layout,
}: UseCanvasNodeDragOptions): UseCanvasNodeDragResult {
  const [dragOverride, setDragOverride] = useState<NodeDragOverride | null>(null);
  const [draggingNodeId, setDraggingNodeId] = useState<string | null>(null);
  const dragRef = useRef<ActiveDrag | null>(null);
  const overrideRef = useRef<NodeDragOverride | null>(null);
  const rafRef = useRef<number | null>(null);
  const transformRef = useRef(transform);
  transformRef.current = transform;

  const onNodeMouseDown = useCallback(
    (e: React.MouseEvent, node: LaidOutNode) => {
      if (!isDraggableNodeKind(node.kind)) return;
      e.stopPropagation();
      e.preventDefault();
      const world = clientToWorld(e.clientX, e.clientY, containerRef.current, transformRef.current);
      if (!world) return;
      const frame = layout.frames.find((f) => f.planId === node.planId);
      if (!frame) return;
      dragRef.current = {
        nodeId: node.id,
        planId: node.planId,
        frameX: frame.box.x,
        frameY: frame.box.y,
        grabOffsetX: world.x - node.box.x,
        grabOffsetY: world.y - node.box.y,
      };
      const initial = { nodeId: node.id, x: node.box.x, y: node.box.y };
      overrideRef.current = initial;
      setDraggingNodeId(node.id);
      setDragOverride(initial);
    },
    [containerRef, layout.frames],
  );

  useEffect(() => {
    if (!draggingNodeId) return;
    const flushOverride = () => {
      rafRef.current = null;
      const last = overrideRef.current;
      if (last) setDragOverride(last);
    };
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const world = clientToWorld(
        e.clientX,
        e.clientY,
        containerRef.current,
        transformRef.current,
      );
      if (!world) return;
      overrideRef.current = {
        nodeId: d.nodeId,
        x: world.x - d.grabOffsetX,
        y: world.y - d.grabOffsetY,
      };
      if (rafRef.current == null) {
        rafRef.current = requestAnimationFrame(flushOverride);
      }
    };
    const onUp = () => {
      if (rafRef.current != null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
        const pending = overrideRef.current;
        if (pending) setDragOverride(pending);
      }
      const d = dragRef.current;
      const last = overrideRef.current;
      dragRef.current = null;
      overrideRef.current = null;
      setDraggingNodeId(null);
      setDragOverride(null);
      if (!d || !last || last.nodeId !== d.nodeId) return;
      const localX = last.x - d.frameX;
      const localY = last.y - d.frameY;
      if (api) {
        void patchPlanLayout(api, d.planId, d.nodeId, localX, localY).catch(() => {});
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [api, containerRef, draggingNodeId]);

  return { dragOverride, draggingNodeId, onNodeMouseDown };
}
