import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { MemoryEdge } from "./MemoryEdge";
import { MemoryNodeDetail } from "./MemoryNodeDetail";
import { BasicNode } from "./nodes/BasicNode";
import { ExpandedNode } from "./nodes/ExpandedNode";
import { ImageNode } from "./nodes/ImageNode";
import type { NodeRendererProps } from "./nodes/NodeRenderer";
import { useViewport } from "./useViewport";
import { usePhysics } from "./usePhysics";
import { deriveNodeKind, KIND_COLORS } from "./nodeKind";
import type { ApiConfig } from "../../lib/types";
import type { MemEdge, MemNode, NodeKind } from "./types";
import type { Pulse } from "./useActivityPulses";
import { getVisibleNodeIds } from "./lib/viewport";
import { RENDERING } from "./lib/rendering-config";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  pulses: Map<string, Pulse>;
  updateNode: (id: string, patch: Partial<MemNode>, markDirty?: boolean) => void;
  bulkUpdatePositions: (updates: Array<{ id: string; x: number; y: number }>, markDirty?: boolean) => void;
  removeNode: (id: string) => void;
  registerFit: (fn: () => void) => void;
  registerRecenter: (fn: () => void) => void;
  selectedIdOverride?: string | null;
  onSelectedIdChange?: (id: string | null) => void;
  physicsEnabled: boolean;
}

const RENDERERS: Record<NodeKind, React.ComponentType<NodeRendererProps>> = {
  basic: BasicNode,
  expanded: ExpandedNode,
  image: ImageNode,
};

export function MemoryCanvas({
  api,
  nodes,
  edges,
  pulses,
  updateNode,
  bulkUpdatePositions,
  removeNode,
  registerFit,
  registerRecenter,
  selectedIdOverride,
  onSelectedIdChange,
  physicsEnabled,
}: Props) {
  const { viewport, onWheel, onPanStart, fitToView, recenter } = useViewport();
  const [selectedIdInternal, setSelectedIdInternal] = useState<string | null>(null);
  const selectedId = selectedIdOverride !== undefined ? selectedIdOverride : selectedIdInternal;
  const setSelectedId = useCallback(
    (id: string | null) => {
      setSelectedIdInternal(id);
      onSelectedIdChange?.(id);
    },
    [onSelectedIdChange],
  );
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const pinnedRef = useRef<Set<string>>(new Set());

  const onPhysicsTick = useCallback(
    (updates: Array<{ id: string; x: number; y: number }>) => {
      bulkUpdatePositions(updates);
    },
    [bulkUpdatePositions],
  );

  const { heat } = usePhysics({
    enabled: physicsEnabled,
    nodes,
    edges,
    pinnedRef,
    onTick: onPhysicsTick,
  });
  const [canvasSize, setCanvasSize] = useState({ width: 0, height: 0 });
  const containerRef = useRef<HTMLDivElement>(null);
  const scaleRef = useRef(viewport.scale);
  scaleRef.current = viewport.scale;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const autoFittedRef = useRef(false);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => setCanvasSize({ width: el.clientWidth, height: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const doFit = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    fitToView(nodesRef.current.values(), el.clientWidth, el.clientHeight);
  }, [fitToView]);

  useEffect(() => {
    registerFit(doFit);
    registerRecenter(recenter);
  }, [registerFit, registerRecenter, doFit, recenter]);

  useEffect(() => {
    if (autoFittedRef.current) return;
    if (nodes.size === 0) return;
    const hasPositions = [...nodes.values()].some(
      (n) => n.graph_x != null && n.graph_y != null,
    );
    if (!hasPositions) return;
    const el = containerRef.current;
    if (!el || el.clientWidth === 0 || el.clientHeight === 0) return;
    autoFittedRef.current = true;
    requestAnimationFrame(() => doFit());
  }, [nodes, doFit, canvasSize]);

  const onDragStart = useCallback(
    (id: string, e: React.MouseEvent) => {
      const startX = e.clientX;
      const startY = e.clientY;
      const cur = nodesRef.current.get(id);
      if (!cur) return;
      const origX = cur.graph_x ?? 0;
      const origY = cur.graph_y ?? 0;
      setSelectedId(id);
      setDraggingId(id);
      pinnedRef.current.add(id);
      heat();
      const move = (ev: MouseEvent) => {
        const dx = (ev.clientX - startX) / scaleRef.current;
        const dy = (ev.clientY - startY) / scaleRef.current;
        updateNode(id, { graph_x: origX + dx, graph_y: origY + dy });
      };
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        setDraggingId(null);
        pinnedRef.current.delete(id);
        heat();
        updateNode(id, {}, true);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
    },
    [updateNode, setSelectedId],
  );

  const onToggleCollapse = useCallback((id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const nodeList = useMemo(() => [...nodes.values()], [nodes]);

  const visibleSet = useMemo(() => {
    if (nodeList.length <= RENDERING.cullThresholdNodes) {
      return new Set(nodeList.map((n) => n.id));
    }
    const ids = getVisibleNodeIds(nodeList, viewport, canvasSize, {
      padding: RENDERING.cullPadding,
    });
    const set = new Set(ids);
    if (draggingId) set.add(draggingId);
    if (selectedId) set.add(selectedId);
    return set;
  }, [nodeList, viewport, canvasSize, draggingId, selectedId]);


  const transform = `translate(${viewport.panX}px, ${viewport.panY}px) scale(${viewport.scale})`;
  const selectedNode = selectedId ? nodes.get(selectedId) ?? null : null;

  const handleDelete = useCallback(
    (id: string) => {
      removeNode(id);
      setSelectedId(null);
    },
    [removeNode, setSelectedId],
  );

  return (
    <div
      ref={containerRef}
      className="absolute inset-0 overflow-hidden cursor-grab bg-[var(--ct-bg)]"
      onWheel={onWheel}
      onMouseDown={(e) => {
        setSelectedId(null);
        onPanStart(e);
      }}
    >
      <div className="absolute inset-0 pointer-events-none">
          <svg
            className="absolute inset-0 w-full h-full pointer-events-none overflow-visible"
          >
            <g style={{ transform, transformOrigin: "0 0" }}>
              {edges.map((e, i) => {
                if (!visibleSet.has(e.from) || !visibleSet.has(e.to)) return null;
                const from = nodes.get(e.from);
                const to = nodes.get(e.to);
                if (!from || !to) return null;
                return (
                  <MemoryEdge
                    key={`${e.from}::${e.rel}::${e.to}::${i}`}
                    edge={e}
                    fromNode={from}
                    toNode={to}
                  />
                );
              })}
            </g>
          </svg>
          <div
            className="absolute inset-0 pointer-events-none"
            style={{ transform, transformOrigin: "0 0" }}
          >
            {nodeList.map((n) => {
              if (!visibleSet.has(n.id)) return null;
              const kind = deriveNodeKind(n);
              const Renderer = RENDERERS[kind];
              return (
                <Renderer
                  key={n.id}
                  node={n}
                  selected={n.id === selectedId}
                  collapsed={collapsed.has(n.id)}
                  scale={viewport.scale}
                  activePulse={pulses.get(n.id) ?? null}
                  onSelect={setSelectedId}
                  onDragStart={onDragStart}
                  onToggleCollapse={onToggleCollapse}
                />
              );
            })}
          </div>
        </div>

      {selectedNode && (
        <MemoryNodeDetail
          api={api}
          node={selectedNode}
          onClose={() => setSelectedId(null)}
          onUpdate={updateNode}
          onDelete={handleDelete}
        />
      )}
      {/* Kind legend */}
      <div className="absolute bottom-3 left-3 flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-[var(--ct-muted-fg)] pointer-events-none opacity-60">
        {Object.entries(KIND_COLORS).map(([kind, color]) => (
          <span key={kind} className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full inline-block" style={{ background: color }} />
            {kind}
          </span>
        ))}
      </div>
    </div>
  );
}
