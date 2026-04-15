import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import EdgeCurveProgram from "@sigma/edge-curve";
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

function nodeSize(n: MemNode): number {
  const imp = n.importance ?? 3;
  return Math.max(3, Math.min(8, 2.5 + imp * 0.6));
}

const SELECTION_COLOR = "#c9d1d9";
const EDGE_RGB = "201,209,217";

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
  const draggingRef = useRef<string | null>(null);

  const [selectedIdInternal, setSelectedIdInternal] = useState<string | null>(null);
  const selectedId =
    selectedIdOverride !== undefined ? selectedIdOverride : selectedIdInternal;
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

  // Keep callbacks referenced by event handlers in a ref so the init effect
  // can run once without re-subscribing when callbacks change.
  const handlersRef = useRef({
    setSelectedId,
    pin: layout.pin,
    setPos: layout.setPosition,
    pinPosition,
  });
  handlersRef.current = {
    setSelectedId,
    pin: layout.pin,
    setPos: layout.setPosition,
    pinPosition,
  };

  // Selection id lives in a ref so the nodeReducer (per-frame closure) sees
  // the latest value without re-wiring sigma.
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selectedId ?? null;
  useEffect(() => {
    sigmaRef.current?.refresh({ skipIndexation: true });
  }, [selectedId]);

  useEffect(() => {
    if (!containerRef.current) return;
    const graph = new Graph({ multi: false });
    graphRef.current = graph;

    const sigma = new Sigma(graph, containerRef.current, {
      renderLabels: true,
      renderEdgeLabels: false,
      labelColor: { color: "#c9d1d9" },
      labelWeight: "400",
      labelSize: 11,
      labelDensity: 0.6,
      labelGridCellSize: 80,
      labelRenderedSizeThreshold: 5,
      minCameraRatio: 0.1,
      maxCameraRatio: 10,
      defaultEdgeType: "curve",
      edgeProgramClasses: { curve: EdgeCurveProgram },
      nodeReducer: (node, data) => {
        if (selectedIdRef.current === node) {
          return {
            ...data,
            highlighted: true,
            color: SELECTION_COLOR,
            size: (data.size as number) + 2,
            zIndex: 1,
          };
        }
        return data;
      },
    });
    sigmaRef.current = sigma;

    sigma.on("clickNode", ({ node }) => handlersRef.current.setSelectedId(node));
    sigma.on("clickStage", () => handlersRef.current.setSelectedId(null));

    sigma.on("downNode", ({ node }) => {
      draggingRef.current = node;
      const attrs = graph.getNodeAttributes(node);
      handlersRef.current.pin(node, {
        x: attrs.x as number,
        y: -(attrs.y as number),
      });
      graph.setNodeAttribute(node, "highlighted", true);
      if (!sigma.getCustomBBox()) sigma.setCustomBBox(sigma.getBBox());
    });

    sigma.on("moveBody", ({ event }) => {
      const id = draggingRef.current;
      if (!id) return;
      const pos = sigma.viewportToGraph(event);
      graph.setNodeAttribute(id, "x", pos.x);
      graph.setNodeAttribute(id, "y", pos.y);
      handlersRef.current.setPos(id, { x: pos.x, y: -pos.y });
      event.preventSigmaDefault();
      event.original.preventDefault();
      event.original.stopPropagation();
    });

    const handleUp = () => {
      const id = draggingRef.current;
      if (!id) return;
      const attrs = graph.getNodeAttributes(id);
      const x = attrs.x as number;
      const y = -(attrs.y as number);
      graph.removeNodeAttribute(id, "highlighted");
      handlersRef.current.pinPosition(id, x, y);
      draggingRef.current = null;
    };
    sigma.on("upNode", handleUp);
    sigma.on("upStage", handleUp);

    return () => {
      sigma.kill();
      sigmaRef.current = null;
      graphRef.current = null;
    };
  }, []);

  // Sync graph topology. Positions come from layout.positions (already
  // seeded by useGraphLayout before this effect fires — hook order).
  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;

    const wanted = new Set(nodes.keys());
    for (const id of graph.nodes()) {
      if (!wanted.has(id)) graph.dropNode(id);
    }
    for (const [id, n] of nodes) {
      const pos = layout.positions.get(id) ?? { x: 0, y: 0 };
      // Sigma's screen y grows downwards; layout uses math-up.
      const attrs = {
        x: pos.x,
        y: -pos.y,
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
          const conf = e.confidence ?? 0.5;
          graph.addEdgeWithKey(key, e.from, e.to, {
            size: 0.1 + conf * 0.2,
            color: `rgba(${EDGE_RGB},${0.06 + conf * 0.14})`,
          });
        } catch {
          /* duplicate — ignore */
        }
      }
    }
    for (const key of graph.edges()) {
      if (!wantedEdges.has(key)) graph.dropEdge(key);
    }
  }, [nodes, edges]);

  // Stream physics ticks into sigma's graphology store. Skip the dragged
  // node so the user's mouse owns it; re-apply on release.
  useEffect(() => {
    const graph = graphRef.current;
    if (!graph) return;
    const dragging = draggingRef.current;
    for (const [id, pos] of layout.positions) {
      if (!graph.hasNode(id)) continue;
      if (id === dragging) continue;
      graph.setNodeAttribute(id, "x", pos.x);
      graph.setNodeAttribute(id, "y", -pos.y);
    }
  }, [layout.version]);

  useEffect(() => {
    const graph = graphRef.current;
    if (!graph) return;
    const timers: number[] = [];
    for (const [id, pulse] of pulses) {
      if (!graph.hasNode(id)) continue;
      const n = nodes.get(id);
      if (!n) continue;
      const base = nodeSize(n);
      graph.mergeNodeAttributes(id, { color: pulse.color, size: base + 1.5 });
      timers.push(
        window.setTimeout(() => {
          const g = graphRef.current;
          if (!g?.hasNode(id)) return;
          g.mergeNodeAttributes(id, { color: colorForKind(n.kind), size: base });
        }, 500),
      );
    }
    return () => {
      for (const t of timers) window.clearTimeout(t);
    };
  }, [pulses, nodes]);

  // Manual fit/recenter buttons — compute bbox + ratio to fit the graph
  // within the container with 15% padding. Ratio=1 would be 1-graph-unit
  // per screen pixel, which blanks the view at our seed layout scale.
  const doFit = useCallback(() => {
    const sigma = sigmaRef.current;
    const graph = graphRef.current;
    const el = containerRef.current;
    if (!sigma || !graph || !el || graph.order === 0) return;
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
    const w = Math.max(1, maxX - minX);
    const h = Math.max(1, maxY - minY);
    const pad = 1.15;
    const ratio = Math.max(
      (w * pad) / Math.max(1, el.clientWidth),
      (h * pad) / Math.max(1, el.clientHeight),
      0.1,
    );
    sigma.getCamera().animate({ x: cx, y: cy, ratio }, { duration: 300 });
  }, []);
  useEffect(() => {
    registerFit(doFit);
    registerRecenter(doFit);
  }, [registerFit, registerRecenter, doFit]);

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
      <div className="absolute bottom-3 left-3 right-3 max-w-[520px] flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-[var(--ct-muted-fg)] pointer-events-none opacity-55 leading-[1.4]">
        {Object.entries(KIND_COLORS).map(([kind, color]) => (
          <span key={kind} className="flex items-center gap-1 whitespace-nowrap">
            <span
              className="w-1.5 h-1.5 rounded-full inline-block shrink-0"
              style={{ background: color }}
            />
            {kind}
          </span>
        ))}
      </div>
    </div>
  );
}
