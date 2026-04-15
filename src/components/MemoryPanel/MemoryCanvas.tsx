import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import type { ApiConfig } from "../../lib/types";
import { MemoryNodeDetail } from "./MemoryNodeDetail";
import { colorForKind, KIND_COLORS } from "./nodeKind";
import { useGraphLayout, type Position } from "./useGraphLayout";
import type { MemEdge, MemNode } from "./types";
import type { Pulse } from "./useActivityPulses";

interface Props {
  api: ApiConfig;
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  pulses: Map<string, Pulse>;
  pinnedPositions: Map<string, Position>;
  updateNode: (id: string, patch: Partial<MemNode>) => void;
  removeNode: (id: string) => void;
  pinPosition: (id: string, x: number, y: number) => void;
  unpinPosition: (id: string) => void;
  registerFit: (fn: () => void) => void;
  registerRecenter: (fn: () => void) => void;
  selectedIdOverride?: string | null;
  onSelectedIdChange?: (id: string | null) => void;
  physicsEnabled: boolean;
}

// Normalize importance [1..10] → node size [4..14].
function nodeSize(n: MemNode): number {
  return Math.max(4, Math.min(14, 3 + (n.importance ?? 3) * 1.1));
}

export function MemoryCanvas({
  api,
  nodes,
  edges,
  pulses,
  pinnedPositions,
  updateNode,
  removeNode,
  pinPosition,
  unpinPosition,
  registerFit,
  registerRecenter,
  selectedIdOverride,
  onSelectedIdChange,
  physicsEnabled,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const dragRef = useRef<{ id: string; active: boolean }>({ id: "", active: false });

  const [selectedIdInternal, setSelectedIdInternal] = useState<string | null>(null);
  const selectedId = selectedIdOverride !== undefined ? selectedIdOverride : selectedIdInternal;
  const setSelectedId = useCallback(
    (id: string | null) => {
      setSelectedIdInternal(id);
      onSelectedIdChange?.(id);
    },
    [onSelectedIdChange],
  );

  const layout = useGraphLayout({
    nodes,
    edges,
    pinnedPositions,
    enabled: physicsEnabled,
  });

  // Sigma init / teardown. Creates one instance bound to the container,
  // tears down on unmount. Graph topology is written in a separate effect
  // so hot-reload and data updates don't thrash the WebGL context.
  useEffect(() => {
    if (!containerRef.current) return;
    const graph = new Graph({ multi: false });
    graphRef.current = graph;
    const sigma = new Sigma(graph, containerRef.current, {
      renderLabels: true,
      renderEdgeLabels: false,
      labelRenderedSizeThreshold: 12,
      defaultNodeType: "circle",
      defaultEdgeType: "line",
    });
    sigmaRef.current = sigma;
    return () => {
      sigma.kill();
      sigmaRef.current = null;
      graphRef.current = null;
    };
  }, []);

  // Wire selection: click on a node = select; click on empty space = deselect.
  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;
    const onClickNode = ({ node }: { node: string }) => setSelectedId(node);
    const onClickStage = () => setSelectedId(null);
    sigma.on("clickNode", onClickNode);
    sigma.on("clickStage", onClickStage);
    return () => {
      sigma.off("clickNode", onClickNode);
      sigma.off("clickStage", onClickStage);
    };
  }, [setSelectedId]);

  // Wire drag: pin on drag-start (so the worker respects the position),
  // emit position updates every move, persist pin on drag-release.
  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;

    const onDownNode = (evt: { node: string; event: { preventSigmaDefault: () => void } }) => {
      dragRef.current = { id: evt.node, active: true };
      const pos = layout.positions.get(evt.node);
      if (pos) layout.pin(evt.node, pos);
      evt.event.preventSigmaDefault();
    };
    const onMove = (evt: { x: number; y: number }) => {
      if (!dragRef.current.active) return;
      const id = dragRef.current.id;
      const graphPos = sigma.viewportToGraph({ x: evt.x, y: evt.y });
      layout.setPosition(id, { x: graphPos.x, y: graphPos.y });
    };
    const onUp = () => {
      if (!dragRef.current.active) return;
      const id = dragRef.current.id;
      const pos = layout.positions.get(id);
      if (pos) pinPosition(id, pos.x, pos.y);
      dragRef.current = { id: "", active: false };
    };

    sigma.on("downNode", onDownNode);
    sigma.getMouseCaptor().on("mousemovebody", onMove);
    sigma.getMouseCaptor().on("mouseup", onUp);
    return () => {
      sigma.off("downNode", onDownNode);
      sigma.getMouseCaptor().off("mousemovebody", onMove);
      sigma.getMouseCaptor().off("mouseup", onUp);
    };
  }, [layout, pinPosition]);

  // Sync graph topology (nodes/edges) when the data source changes. Positions
  // get filled in from the layout cache when available, otherwise Sigma puts
  // the node at (0,0) and the next layout tick moves it.
  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;

    const currentNodeIds = new Set(graph.nodes());
    const wantedNodeIds = new Set(nodes.keys());

    for (const id of currentNodeIds) {
      if (!wantedNodeIds.has(id)) graph.dropNode(id);
    }
    for (const [id, n] of nodes) {
      const pos = layout.positions.get(id) ?? { x: 0, y: 0 };
      const attrs = {
        x: pos.x,
        y: -pos.y, // Sigma's y is screen-down, layout uses math-up
        size: nodeSize(n),
        color: colorForKind(n.kind),
        label: n.title,
      };
      if (graph.hasNode(id)) graph.mergeNodeAttributes(id, attrs);
      else graph.addNode(id, attrs);
    }

    const wantedEdges = new Set<string>();
    for (const e of edges) {
      if (!graph.hasNode(e.from) || !graph.hasNode(e.to)) continue;
      const key = `${e.from}→${e.to}`;
      wantedEdges.add(key);
      if (!graph.hasEdge(key)) {
        try {
          graph.addEdgeWithKey(key, e.from, e.to, {
            size: 0.4 + (e.confidence ?? 0.5) * 0.8,
            color: `rgba(255,255,255,${0.12 + (e.confidence ?? 0.5) * 0.35})`,
          });
        } catch {
          /* duplicate */
        }
      }
    }
    for (const key of graph.edges()) {
      if (!wantedEdges.has(key)) graph.dropEdge(key);
    }
    sigma.refresh();
  }, [nodes, edges, layout.positions]);

  // Stream physics ticks back into Sigma.
  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;
    for (const [id, pos] of layout.positions) {
      if (graph.hasNode(id)) {
        graph.setNodeAttribute(id, "x", pos.x);
        graph.setNodeAttribute(id, "y", -pos.y);
      }
    }
    sigma.refresh();
  }, [layout.version]);

  // Pulse rendering: bump the matching node's size briefly. Cheap — Sigma
  // redraws on attribute change, no extra layers.
  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;
    for (const [id, _pulse] of pulses) {
      if (!graph.hasNode(id)) continue;
      const n = nodes.get(id);
      if (!n) continue;
      graph.setNodeAttribute(id, "size", nodeSize(n) * 1.4);
    }
    sigma.refresh();
    const t = window.setTimeout(() => {
      if (!graphRef.current) return;
      for (const [id] of pulses) {
        if (!graph.hasNode(id)) continue;
        const n = nodes.get(id);
        if (!n) continue;
        graph.setNodeAttribute(id, "size", nodeSize(n));
      }
      sigmaRef.current?.refresh();
    }, 600);
    return () => window.clearTimeout(t);
  }, [pulses, nodes]);

  // Fit / recenter registration (MemoryPanel wires buttons to these).
  const doFit = useCallback(() => {
    const sigma = sigmaRef.current;
    const graph = graphRef.current;
    if (!sigma || !graph || graph.order === 0) return;
    // Compute bbox of all node positions in graph space.
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    graph.forEachNode((_id, attrs) => {
      const x = attrs.x as number;
      const y = attrs.y as number;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    });
    if (!isFinite(minX)) return;
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    const camera = sigma.getCamera();
    camera.animate({ x: cx, y: cy, ratio: 1 }, { duration: 300 });
  }, []);
  const doRecenter = useCallback(() => {
    sigmaRef.current?.getCamera().animate({ x: 0, y: 0, ratio: 1 }, { duration: 300 });
  }, []);
  useEffect(() => {
    registerFit(doFit);
    registerRecenter(doRecenter);
  }, [registerFit, registerRecenter, doFit, doRecenter]);

  // First-load auto-fit after positions settle.
  const autoFittedRef = useRef(false);
  useEffect(() => {
    if (autoFittedRef.current) return;
    if (nodes.size === 0 || layout.positions.size === 0) return;
    autoFittedRef.current = true;
    requestAnimationFrame(doFit);
  }, [nodes.size, layout.positions.size, doFit]);

  const selectedNode = selectedId ? nodes.get(selectedId) ?? null : null;

  const handleDelete = useCallback(
    (id: string) => {
      removeNode(id);
      setSelectedId(null);
    },
    [removeNode, setSelectedId],
  );

  const handleUnpin = useCallback(() => {
    if (selectedId) unpinPosition(selectedId);
  }, [selectedId, unpinPosition]);

  const isSelectedPinned = useMemo(
    () => (selectedId ? pinnedPositions.has(selectedId) : false),
    [selectedId, pinnedPositions],
  );

  return (
    <div className="absolute inset-0 overflow-hidden bg-[var(--ct-bg)]">
      <div ref={containerRef} className="absolute inset-0" />

      {selectedNode && (
        <MemoryNodeDetail
          api={api}
          node={selectedNode}
          onClose={() => setSelectedId(null)}
          onUpdate={updateNode}
          onDelete={handleDelete}
          isPinned={isSelectedPinned}
          onUnpin={isSelectedPinned ? handleUnpin : undefined}
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
