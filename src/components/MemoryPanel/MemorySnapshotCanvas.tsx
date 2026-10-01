import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import EdgeCurveProgram from "@sigma/edge-curve";
import type { ApiConfig } from "../../lib/types";
import { MemoryNodeDetail } from "./MemoryNodeDetail";
import { colorForKind, KIND_COLORS } from "./nodeKind";
import { fetchGraphEdges, fetchLabels, setPinnedPosition } from "./memoryApi";
import {
  readMemoryGraphPalette,
  type MemoryGraphPalette,
} from "./memoryGraphTheme";
import type { MemEdge, MemNode } from "./types";
import type { Pulse } from "./useActivityPulses";
import type { NodeIndex } from "./snapshotLogic";
import {
  EDGE_DETAIL_RATIO,
  edgeCandidates,
  inViewport,
  isZoomedOut,
  labelCandidates,
  overviewNodes,
  SMALL_GRAPH_THRESHOLD,
  type Bounds,
} from "./snapshotLogic";
import type { SnapshotEdge, SnapshotNode } from "./snapshotTypes";

interface Props {
  api: ApiConfig;
  index: NodeIndex;
  pulses: Map<string, Pulse>;
  registerFit: (fn: () => void) => void;
  registerRecenter: (fn: () => void) => void;
  epoch?: number;
  selectedIdOverride?: string | null;
  onSelectedIdChange?: (id: string | null) => void;
}

const SELECTION_COLOR = "#c9d1d9";
const CLUSTER_COLOR = "#5b6472";
const GHOST_COLOR = "rgba(91,100,114,0.08)";
const LOD_DEBOUNCE_MS = 120;

function nodeSize(importance: number): number {
  return Math.max(3, Math.min(8, 2.5 + importance * 0.6));
}

function clusterSize(size: number): number {
  return Math.max(6, Math.min(40, 6 + Math.sqrt(size) * 2));
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

function clusterNodeKey(id: number): string {
  return `__cluster_${id}`;
}

export function MemorySnapshotCanvas({
  api,
  index,
  pulses,
  registerFit,
  registerRecenter,
  epoch,
  selectedIdOverride,
  onSelectedIdChange,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const draggingRef = useRef<string | null>(null);
  const indexRef = useRef<NodeIndex>(index);
  indexRef.current = index;

  // Lazy caches: titles, real edges per node, and which nodes we already asked for.
  const labelsRef = useRef<Map<string, string>>(new Map());
  const fetchedLabelsRef = useRef<Set<string>>(new Set());
  const edgesByNodeRef = useRef<Map<string, SnapshotEdge[]>>(new Map());
  const fetchedEdgesRef = useRef<Set<string>>(new Set());
  const paletteRef = useRef<MemoryGraphPalette | null>(null);
  const [labelTick, setLabelTick] = useState(0);
  const [egoEdges, setEgoEdges] = useState<SnapshotEdge[]>([]);
  // Selected node + its 1-hop neighborhood; everything else renders ghosted.
  const egoSetRef = useRef<Set<string> | null>(null);

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
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selectedId ?? null;
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const hoveredIdRef = useRef<string | null>(null);
  hoveredIdRef.current = hoveredId;

  const handlersRef = useRef({ setSelectedId });
  handlersRef.current = { setSelectedId };

  const [chipStyle, setChipStyle] = useState<{
    x: number;
    y: number;
    label: string;
    sub: string;
  } | null>(null);

  useEffect(() => {
    sigmaRef.current?.refresh({ skipIndexation: true });
  }, [selectedId, hoveredId]);

  // Sigma init — same options as the legacy canvas.
  useEffect(() => {
    if (!containerRef.current) return;
    paletteRef.current = readMemoryGraphPalette();
    const graph = new Graph({ multi: false });
    graphRef.current = graph;
    const palette = paletteRef.current;

    const sigma = new Sigma(graph, containerRef.current, {
      renderLabels: true,
      renderEdgeLabels: false,
      labelColor: { color: palette.labelColor },
      labelWeight: "400",
      labelSize: 11,
      labelDensity: 0.6,
      labelGridCellSize: 80,
      labelRenderedSizeThreshold: 5,
      minCameraRatio: 0.05,
      maxCameraRatio: 20,
      defaultEdgeType: "curve",
      edgeProgramClasses: { curve: EdgeCurveProgram },
      nodeReducer: (node, data) => {
        const isActive =
          selectedIdRef.current === node || hoveredIdRef.current === node;
        if (isActive) {
          return {
            ...data,
            color: SELECTION_COLOR,
            size: (data.size as number) + 1,
            label: "",
            zIndex: 1,
          };
        }
        const ego = egoSetRef.current;
        if (ego && !ego.has(node)) {
          return { ...data, color: GHOST_COLOR, label: "", zIndex: 0 };
        }
        const label = (data.label as string) ?? "";
        if (ego) {
          // 1-hop neighbor: full color, show its title when we have it.
          return { ...data, label: labelsRef.current.get(node) ?? label, zIndex: 1 };
        }
        // Cluster aggregates keep their label; item labels appear only when fetched.
        return { ...data, label: data.isCluster ? label : "" };
      },
      edgeReducer: (edge, data) => {
        const sel = selectedIdRef.current;
        if (!sel || !egoSetRef.current) return data;
        const g = graphRef.current;
        if (!g) return data;
        if (!g.extremities(edge).includes(sel)) return { ...data, hidden: true };
        const palette = paletteRef.current;
        if (!palette) return { ...data, zIndex: 1 };
        const size = Math.max(data.size ?? 0, 0.4);
        return { ...data, zIndex: 1, size, color: palette.edgeColor(palette.egoEdgeAlpha) };
      },
    });
    sigmaRef.current = sigma;

    sigma.on("clickNode", ({ node }) => {
      if (node.startsWith("__cluster_")) {
        // Zoom into the cluster centroid.
        const attrs = graph.getNodeAttributes(node);
        sigma.getCamera().animate(
          { x: attrs.x as number, y: attrs.y as number, ratio: 0.4 },
          { duration: 350 },
        );
        return;
      }
      handlersRef.current.setSelectedId(node);
    });
    sigma.on("clickStage", () => handlersRef.current.setSelectedId(null));
    sigma.on("enterNode", ({ node }) => {
      if (!node.startsWith("__cluster_")) setHoveredId(node);
    });
    sigma.on("leaveNode", () => setHoveredId(null));

    sigma.on("downNode", ({ node }) => {
      if (node.startsWith("__cluster_")) return;
      draggingRef.current = node;
      if (!sigma.getCustomBBox()) sigma.setCustomBBox(sigma.getBBox());
    });

    sigma.on("moveBody", ({ event }) => {
      const id = draggingRef.current;
      if (!id) return;
      const pos = sigma.viewportToGraph(event);
      graph.setNodeAttribute(id, "x", pos.x);
      graph.setNodeAttribute(id, "y", pos.y);
      event.preventSigmaDefault();
      event.original.preventDefault();
      event.original.stopPropagation();
    });

    const handleUp = () => {
      const id = draggingRef.current;
      if (!id) return;
      draggingRef.current = null;
      const attrs = graph.getNodeAttributes(id);
      // Persist drag as a pin (graph y is screen-down; store math-up).
      setPinnedPosition(api, id, attrs.x as number, -(attrs.y as number)).catch(() => {});
    };
    sigma.on("upNode", handleUp);
    sigma.on("upStage", handleUp);

    return () => {
      sigma.kill();
      sigmaRef.current = null;
      graphRef.current = null;
    };
  }, [api.host, api.port, api.token]);

  const adapterNodes = useMemo(() => buildAdapterMap(index, labelsRef.current), [index, labelTick]);

  const rebuildGraph = useCallback(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;
    paletteRef.current = readMemoryGraphPalette();
    const palette = paletteRef.current;
    const idx = indexRef.current;
    const ratio = sigma.getCamera().ratio;
    const zoomedOut = isZoomedOut(ratio, idx.byId.size);
    const hadExtent = graph.order > 0;

    graph.clear();

    if (zoomedOut) {
      buildClusterView(graph, idx, palette);
      sigma.refresh();
      return;
    }
    // and on the first build, when viewportToGraph has nothing to map against.
    const small = idx.byId.size <= SMALL_GRAPH_THRESHOLD;
    const bounds = small || !hadExtent ? null : viewportBounds(sigma);
    buildDetailView(graph, idx, bounds, labelsRef.current);
    // Mass edges only when zoomed in deep — the overview stays a clean map;
    // the selected node's ego edges are always visible.
    const showEdges = ratio <= EDGE_DETAIL_RATIO;
    if (showEdges) applyRealEdges(graph, edgesByNodeRef.current, palette);
    applyEgoEdges(graph, selectedIdRef.current, edgesByNodeRef.current, palette);
    sigma.refresh();
    void fetchViewportData(api, idx, bounds ?? viewportBounds(sigma), showEdges);
  }, [api.host, api.port, api.token]);

  // Lazy fetch for the viewport: labels + edges, dedupe, cache, max 500/req.
  const fetchViewportData = useCallback(
    async (a: ApiConfig, idx: NodeIndex, bounds: Bounds, wantEdges: boolean) => {
      const nodes = [...idx.byId.values()];
      const labelIds = labelCandidates(nodes, bounds, fetchedLabelsRef.current);
      const edgeIds = wantEdges ? edgeCandidates(nodes, bounds, fetchedEdgesRef.current) : [];
      if (labelIds.length) {
        for (const id of labelIds) fetchedLabelsRef.current.add(id);
        try {
          const rows = await fetchLabels(a, labelIds);
          for (const r of rows) labelsRef.current.set(r.id, r.title);
          if (rows.length) {
            setLabelTick((t) => t + 1);
            applyLabels(graphRef.current, rows);
            sigmaRef.current?.refresh({ skipIndexation: true });
          }
        } catch {
          /* retry on next LOD pass */
        }
      }
      if (edgeIds.length) {
        for (const id of edgeIds) fetchedEdgesRef.current.add(id);
        try {
          const rows = await fetchGraphEdges(a, edgeIds);
          mergeEdges(edgesByNodeRef.current, rows);
          applyRealEdges(
            graphRef.current,
            edgesByNodeRef.current,
            paletteRef.current ?? readMemoryGraphPalette(),
          );
          sigmaRef.current?.refresh({ skipIndexation: true });
        } catch {
          /* retry on next LOD pass */
        }
      }
    },
    [],
  );

  // A full snapshot can change any node's edges or title; drop the lazy caches before rebuilding.
  const cacheEpochRef = useRef(epoch);
  useEffect(() => {
    if (cacheEpochRef.current !== epoch) {
      cacheEpochRef.current = epoch;
      labelsRef.current.clear();
      fetchedLabelsRef.current.clear();
      edgesByNodeRef.current.clear();
      fetchedEdgesRef.current.clear();
    }
    rebuildGraph();
  }, [index, epoch, rebuildGraph]);

  // Rebuild on camera move (LOD swap), debounced.
  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;
    let timer: number | null = null;
    const onMove = () => {
      if (timer != null) window.clearTimeout(timer);
      timer = window.setTimeout(() => rebuildGraph(), LOD_DEBOUNCE_MS);
    };
    sigma.getCamera().on("updated", onMove);
    return () => {
      sigma.getCamera().off("updated", onMove);
      if (timer != null) window.clearTimeout(timer);
    };
  }, [rebuildGraph]);

  // Selection + its 1-hop neighborhood; everything outside renders ghosted.
  useEffect(() => {
    if (!selectedId) {
      egoSetRef.current = null;
    } else {
      const set = new Set<string>([selectedId]);
      for (const e of egoEdges) {
        set.add(e.from);
        set.add(e.to);
      }
      egoSetRef.current = set;
    }
    sigmaRef.current?.refresh({ skipIndexation: true });
  }, [selectedId, egoEdges]);

  // Selected node → always fetch + show its ego edges.
  useEffect(() => {
    if (!selectedId) {
      setEgoEdges([]);
      return;
    }
    let active = true;
    fetchGraphEdges(api, [selectedId])
      .then((rows) => {
        if (!active) return;
        setEgoEdges(rows);
        mergeEdges(edgesByNodeRef.current, rows);
        applyEgoEdges(
          graphRef.current,
          selectedId,
          edgesByNodeRef.current,
          paletteRef.current ?? readMemoryGraphPalette(),
        );
        sigmaRef.current?.refresh({ skipIndexation: true });
      })
      .catch(() => {});
    // Fetch the selected node's own label if missing.
    if (!labelsRef.current.has(selectedId)) {
      fetchLabels(api, [selectedId])
        .then((rows) => {
          if (!active || !rows.length) return;
          for (const r of rows) labelsRef.current.set(r.id, r.title);
          setLabelTick((t) => t + 1);
        })
        .catch(() => {});
    }
    return () => { active = false; };
  }, [selectedId, api.host, api.port, api.token]);

  // Pulse animation — same as legacy canvas.
  useEffect(() => {
    const graph = graphRef.current;
    if (!graph) return;
    const timers: number[] = [];
    for (const [id, pulse] of pulses) {
      if (!graph.hasNode(id)) continue;
      const n = indexRef.current.byId.get(id);
      if (!n) continue;
      const base = nodeSize(n.importance);
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
  }, [pulses]);

  // Active-node chip overlay.
  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;
    const onAfter = () => {
      const activeId = selectedIdRef.current ?? hoveredIdRef.current;
      if (!activeId) {
        setChipStyle((prev) => (prev == null ? prev : null));
        return;
      }
      const n = indexRef.current.byId.get(activeId);
      const graph = graphRef.current;
      if (!n || !graph?.hasNode(activeId)) return;
      const attrs = graph.getNodeAttributes(activeId);
      const viewport = sigma.graphToViewport({ x: attrs.x as number, y: attrs.y as number });
      const title = labelsRef.current.get(activeId) ?? activeId;
      setChipStyle((prev) => {
        const next = {
          x: viewport.x,
          y: viewport.y,
          label: truncate(title, 60),
          sub: n.kind,
        };
        if (
          prev &&
          Math.abs(prev.x - next.x) < 0.5 &&
          Math.abs(prev.y - next.y) < 0.5 &&
          prev.label === next.label &&
          prev.sub === next.sub
        ) {
          return prev;
        }
        return next;
      });
    };
    sigma.on("afterRender", onAfter);
    onAfter();
    return () => { sigma.off("afterRender", onAfter); };
  }, [selectedId, hoveredId, labelTick]);

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
      0.05,
    );
    sigma.getCamera().animate({ x: cx, y: cy, ratio }, { duration: 300 });
  }, []);
  useEffect(() => {
    registerFit(doFit);
    registerRecenter(doFit);
  }, [registerFit, registerRecenter, doFit]);

  const selectedNode = selectedId ? adapterNodes.get(selectedId) ?? null : null;
  const detailEdges: MemEdge[] = useMemo(
    () =>
      egoEdges.map((e) => ({
        from: e.from,
        to: e.to,
        rel: e.rel,
        confidence: e.confidence,
        valid_from: null,
        valid_to: null,
      })),
    [egoEdges],
  );

  const handleDelete = useCallback(
    (id: string) => {
      graphRef.current?.dropNode(id);
      setSelectedId(null);
    },
    [setSelectedId],
  );

  return (
    <div className="absolute inset-0 overflow-hidden bg-[var(--ct-bg)]">
      <div ref={containerRef} className="absolute inset-0" />

      {chipStyle && (
        <div
          className="absolute pointer-events-none select-none px-2 py-1 rounded-md max-w-[280px]"
          style={{
            left: chipStyle.x,
            top: chipStyle.y,
            transform: "translate(12px, -50%)",
            background: SELECTION_COLOR,
            color: "#111",
            boxShadow: "0 1px 2px rgba(0,0,0,0.3)",
          }}
        >
          <div className="text-[11px] font-medium leading-tight break-words">
            {chipStyle.label}
          </div>
          <div className="text-[9px] opacity-70 leading-tight mt-0.5">{chipStyle.sub}</div>
        </div>
      )}

      {selectedNode && (
        <MemoryNodeDetail
          api={api}
          node={selectedNode}
          onUpdate={() => {}}
          onDelete={handleDelete}
          edges={detailEdges}
          nodes={adapterNodes}
          onSelectNode={setSelectedId}
        />
      )}

      <div className="absolute top-2 left-2 max-w-[520px] flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-[var(--ct-muted-fg)] pointer-events-none opacity-55 leading-[1.4]">
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

function buildAdapterMap(index: NodeIndex, labels: Map<string, string>): Map<string, MemNode> {
  const out = new Map<string, MemNode>();
  for (const [id, n] of index.byId) {
    out.set(id, {
      id,
      kind: n.kind,
      title: labels.get(id) ?? id,
      summary: null,
      area: null,
      importance: n.importance,
      tags: null,
      has_vector: n.has_vector,
      has_body: true,
    });
  }
  return out;
}

function viewportBounds(sigma: Sigma): Bounds {
  const { width, height } = sigma.getDimensions();
  const tl = sigma.viewportToGraph({ x: 0, y: 0 });
  const br = sigma.viewportToGraph({ x: width, y: height });
  return {
    minX: Math.min(tl.x, br.x),
    maxX: Math.max(tl.x, br.x),
    minY: Math.min(-tl.y, -br.y),
    maxY: Math.max(-tl.y, -br.y),
  };
}

function addItemNode(graph: Graph, n: SnapshotNode, label: string) {
  graph.addNode(n.id, {
    x: n.x,
    y: -n.y,
    size: nodeSize(n.importance),
    color: colorForKind(n.kind),
    label,
    isCluster: false,
  });
}

function buildClusterView(graph: Graph, idx: NodeIndex, palette: MemoryGraphPalette) {
  for (const c of idx.clusters) {
    graph.addNode(clusterNodeKey(c.id), {
      x: c.x,
      y: -c.y,
      size: clusterSize(c.size),
      color: CLUSTER_COLOR,
      label: `${c.size}`,
      isCluster: true,
    });
  }
  for (const n of overviewNodes([...idx.byId.values()])) {
    if (!graph.hasNode(n.id)) addItemNode(graph, n, "");
  }
  for (const e of idx.clusterEdges) {
    const from = clusterNodeKey(e.from);
    const to = clusterNodeKey(e.to);
    if (!graph.hasNode(from) || !graph.hasNode(to)) continue;
    const key = `c${e.from}-${e.to}`;
    if (graph.hasEdge(key)) continue;
    try {
      graph.addEdgeWithKey(key, from, to, {
        size: Math.min(2, 0.3 + Math.log10(e.weight + 1) * 0.6),
        color: palette.edgeColor(palette.clusterEdgeAlpha),
      });
    } catch {
      /* dup */
    }
  }
}

function buildDetailView(
  graph: Graph,
  idx: NodeIndex,
  bounds: Bounds | null,
  labels: Map<string, string>,
) {
  for (const n of idx.byId.values()) {
    if (!inViewport(n, bounds)) continue;
    addItemNode(graph, n, labels.get(n.id) ?? "");
  }
}

function applyLabels(graph: Graph | null, rows: { id: string; title: string }[]) {
  if (!graph) return;
  for (const r of rows) {
    if (graph.hasNode(r.id)) graph.setNodeAttribute(r.id, "label", r.title);
  }
}

function mergeEdges(store: Map<string, SnapshotEdge[]>, rows: SnapshotEdge[]) {
  for (const e of rows) {
    for (const end of [e.from, e.to]) {
      const list = store.get(end) ?? [];
      if (!list.some((x) => x.from === e.from && x.to === e.to && x.rel === e.rel)) {
        list.push(e);
        store.set(end, list);
      }
    }
  }
}

function applyRealEdges(
  graph: Graph | null,
  store: Map<string, SnapshotEdge[]>,
  palette: MemoryGraphPalette,
) {
  if (!graph) return;
  const seen = new Set<string>();
  for (const list of store.values()) {
    for (const e of list) {
      const key = `${e.from}→${e.to}→${e.rel}`;
      if (seen.has(key)) continue;
      seen.add(key);
      if (!graph.hasNode(e.from) || !graph.hasNode(e.to) || graph.hasEdge(key)) continue;
      try {
        const conf = e.confidence ?? 0.5;
        graph.addEdgeWithKey(key, e.from, e.to, {
          size: 0.005 + conf * 0.01,
          color: palette.edgeColor(palette.realEdgeAlpha(conf)),
        });
      } catch {
        /* dup */
      }
    }
  }
}

function applyEgoEdges(
  graph: Graph | null,
  selectedId: string | null,
  store: Map<string, SnapshotEdge[]>,
  palette: MemoryGraphPalette,
) {
  if (!graph || !selectedId) return;
  const list = store.get(selectedId) ?? [];
  for (const e of list) {
    const key = `${e.from}→${e.to}→${e.rel}`;
    if (!graph.hasNode(e.from) || !graph.hasNode(e.to) || graph.hasEdge(key)) continue;
    try {
      graph.addEdgeWithKey(key, e.from, e.to, {
        size: 0.4,
        color: palette.edgeColor(palette.egoEdgeAlpha),
      });
    } catch {
      /* dup */
    }
  }
}
