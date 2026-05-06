import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import EdgeCurveProgram from "@sigma/edge-curve";
import { clusterKeyFromPath, colorForCluster, colorForKind, edgeRgbFor, sizeFromImportance } from "./styles";
import type { ProjectMapEdge, ProjectMapNode } from "./types";

interface Props {
  nodes: ProjectMapNode[];
  edges: ProjectMapEdge[];
  visibleKinds: Set<string>;
  focusId: number | null;
  onFocus: (id: number | null) => void;
  registerFit?: (fn: () => void) => void;
  selected?: Set<number>;
  onSelectToggle?: (id: number, additive: boolean) => void;
  onSelectionClear?: () => void;
  layoutOverride?: Map<number, { x: number; y: number }> | null;
}

const NODE_BASE_SIZE = 3.5;

function nodeSize(kind: string): number {
  if (kind === "file" || kind === "module") return NODE_BASE_SIZE + 2.5;
  if (kind === "class") return NODE_BASE_SIZE + 1.5;
  return NODE_BASE_SIZE;
}

function nodeColor(node: ProjectMapNode): string {
  if (node.kind === "file" || node.kind === "module") {
    return colorForCluster(clusterKeyFromPath(node.path));
  }
  return colorForKind(node.kind);
}

function deterministicSeed(id: number): { x: number; y: number } {
  const a = Math.sin(id * 12.9898) * 43758.5453;
  const b = Math.sin(id * 78.233) * 43758.5453;
  return { x: a - Math.floor(a), y: b - Math.floor(b) };
}

export function ProjectMapCanvas({
  nodes,
  edges,
  visibleKinds,
  focusId,
  onFocus,
  registerFit,
  selected,
  onSelectToggle,
  onSelectionClear,
  layoutOverride,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const focusIdRef = useRef<number | null>(focusId);
  focusIdRef.current = focusId;
  const visibleKindsRef = useRef(visibleKinds);
  visibleKindsRef.current = visibleKinds;

  const [hoverId, setHoverId] = useState<number | null>(null);
  const hoverIdRef = useRef<number | null>(null);
  hoverIdRef.current = hoverId;

  const selectedRef = useRef<Set<number> | null>(null);
  selectedRef.current = selected ?? null;
  const onSelectToggleRef = useRef(onSelectToggle);
  onSelectToggleRef.current = onSelectToggle;
  const onSelectionClearRef = useRef(onSelectionClear);
  onSelectionClearRef.current = onSelectionClear;

  const neighbourSet = useMemo(() => {
    if (focusId == null) return null;
    const set = new Set<number>([focusId]);
    for (const e of edges) {
      if (e.source === focusId) set.add(e.target);
      if (e.target === focusId) set.add(e.source);
    }
    return set;
  }, [focusId, edges]);
  const neighbourRef = useRef<Set<number> | null>(null);
  neighbourRef.current = neighbourSet;

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
      labelDensity: 0.4,
      labelGridCellSize: 80,
      labelRenderedSizeThreshold: 6,
      minCameraRatio: 0.05,
      maxCameraRatio: 12,
      defaultEdgeType: "curve",
      edgeProgramClasses: { curve: EdgeCurveProgram },
      nodeReducer: (node, data) => {
        const id = Number(node);
        const kind = (data.kind as string) ?? "";
        const visible = visibleKindsRef.current.size === 0 || visibleKindsRef.current.has(kind);
        const inFocus = neighbourRef.current ? neighbourRef.current.has(id) : true;
        const isActive = focusIdRef.current === id || hoverIdRef.current === id;
        const isSelected = selectedRef.current?.has(id) ?? false;
        const isFile = kind === "file" || kind === "module";
        if (!visible) {
          return { ...data, hidden: true };
        }
        if (neighbourRef.current === null && !isFile && !isSelected) {
          return { ...data, hidden: true };
        }
        if (!inFocus && !isSelected) {
          return { ...data, hidden: true };
        }
        if (isSelected) {
          return {
            ...data,
            color: "#f5d77a",
            size: (data.size as number) + 1.8,
            zIndex: 2,
          };
        }
        if (isActive) {
          return {
            ...data,
            color: "#f5f5f5",
            size: (data.size as number) + 1.5,
            zIndex: 1,
          };
        }
        return data;
      },
      edgeReducer: (edge, data) => {
        const kind = (data.kind as string) ?? "invoke";
        const [s, t] = graph.extremities(edge);
        const sId = Number(s);
        const tId = Number(t);
        const isAggregate = kind.startsWith("file_");
        if (neighbourRef.current === null) {
          if (!isAggregate) {
            return { ...data, hidden: true };
          }
          return data;
        }
        if (isAggregate) {
          return { ...data, hidden: true };
        }
        const inFocus = neighbourRef.current.has(sId) && neighbourRef.current.has(tId);
        if (!inFocus) {
          return { ...data, hidden: true };
        }
        return data;
      },
    });
    sigmaRef.current = sigma;
    sigma.on("clickNode", ({ node, event }) => {
      const id = Number(node);
      const additive = !!(event?.original as MouseEvent | undefined)?.metaKey
        || !!(event?.original as MouseEvent | undefined)?.ctrlKey;
      const toggle = onSelectToggleRef.current;
      if (additive && toggle) {
        toggle(id, true);
        return;
      }
      onFocus(id);
      if (toggle) toggle(id, false);
    });
    sigma.on("clickStage", () => {
      onFocus(null);
      onSelectionClearRef.current?.();
    });
    sigma.on("enterNode", ({ node }) => setHoverId(Number(node)));
    sigma.on("leaveNode", () => setHoverId(null));
    return () => {
      sigma.kill();
      sigmaRef.current = null;
      graphRef.current = null;
    };
  }, [onFocus]);

  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;

    let minImp = Infinity;
    let maxImp = -Infinity;
    for (const n of nodes) {
      if (typeof n.importance === "number") {
        if (n.importance < minImp) minImp = n.importance;
        if (n.importance > maxImp) maxImp = n.importance;
      }
    }
    if (!isFinite(minImp)) {
      minImp = 0;
      maxImp = 1;
    }

    const wantedNodes = new Set<string>();
    const clusterBuckets = new Map<string, number>();
    for (const n of nodes) {
      const idStr = String(n.id);
      wantedNodes.add(idStr);
      const seed = deterministicSeed(n.id);
      const cluster = clusterKeyFromPath(n.path);
      let bucket = clusterBuckets.get(cluster);
      if (bucket === undefined) {
        bucket = clusterBuckets.size;
        clusterBuckets.set(cluster, bucket);
      }
      const clusterAngle = (bucket * 2.39996323) % (Math.PI * 2);
      const clusterRadius = 8 + Math.sqrt(bucket + 1) * 2.0;
      const cx = Math.cos(clusterAngle) * clusterRadius;
      const cy = Math.sin(clusterAngle) * clusterRadius;
      const override = layoutOverride?.get(n.id);
      const x = override ? override.x : cx + (seed.x - 0.5) * 2.4;
      const y = override ? override.y : cy + (seed.y - 0.5) * 2.4;
      const importanceSize = sizeFromImportance(n.importance, minImp, maxImp);
      const baseSize = nodeSize(n.kind);
      const size = n.kind === "file" || n.kind === "module" ? Math.max(baseSize, importanceSize) : baseSize;
      const attrs = {
        x,
        y,
        size,
        color: nodeColor(n),
        label: n.display_name,
        kind: n.kind,
        cluster,
      };
      if (graph.hasNode(idStr)) graph.mergeNodeAttributes(idStr, attrs);
      else graph.addNode(idStr, attrs);
    }
    for (const id of graph.nodes()) {
      if (!wantedNodes.has(id)) graph.dropNode(id);
    }

    const fileNodeByPath = new Map<string, number>();
    const fileNodeByDef = new Map<number, number>();
    for (const n of nodes) {
      if ((n.kind === "file" || n.kind === "module") && n.path) {
        fileNodeByPath.set(n.path, n.id);
      }
    }
    for (const n of nodes) {
      if (n.kind === "file" || n.kind === "module") continue;
      if (!n.path) continue;
      const fid = fileNodeByPath.get(n.path);
      if (fid != null) fileNodeByDef.set(n.id, fid);
    }

    const aggregatedFileEdges = new Map<string, { src: number; tgt: number; kind: string; weight: number }>();

    const wantedEdges = new Set<string>();
    for (const e of edges) {
      const key = `${e.source}->${e.target}-${e.kind}`;
      const sId = String(e.source);
      const tId = String(e.target);
      if (!graph.hasNode(sId) || !graph.hasNode(tId)) continue;
      wantedEdges.add(key);
      if (!graph.hasEdge(key)) {
        const rgb = edgeRgbFor(e.kind);
        const alpha = e.kind === "contain" ? 0.18 : 0.32;
        try {
          graph.addEdgeWithKey(key, sId, tId, {
            size: e.kind === "contain" ? 0.04 : 0.08,
            color: `rgba(${rgb},${alpha})`,
            kindRgb: rgb,
            kind: e.kind,
          });
        } catch {
          /* duplicate */
        }
      }
      if (e.kind === "invoke" || e.kind === "bridge") {
        const sFile = fileNodeByDef.get(e.source) ?? (
          (nodes.find((n) => n.id === e.source)?.kind === "file" ||
            nodes.find((n) => n.id === e.source)?.kind === "module")
            ? e.source
            : null
        );
        const tFile = fileNodeByDef.get(e.target) ?? (
          (nodes.find((n) => n.id === e.target)?.kind === "file" ||
            nodes.find((n) => n.id === e.target)?.kind === "module")
            ? e.target
            : null
        );
        if (sFile != null && tFile != null && sFile !== tFile) {
          const aggKey = `${sFile}->${tFile}-file_${e.kind}`;
          const existing = aggregatedFileEdges.get(aggKey);
          if (existing) {
            existing.weight += e.weight;
          } else {
            aggregatedFileEdges.set(aggKey, { src: sFile, tgt: tFile, kind: `file_${e.kind}`, weight: e.weight });
          }
        }
      }
    }
    for (const [key, agg] of aggregatedFileEdges) {
      wantedEdges.add(key);
      if (graph.hasEdge(key)) continue;
      const baseKind = agg.kind === "file_bridge" ? "bridge" : "invoke";
      const rgb = edgeRgbFor(baseKind);
      const alpha = Math.min(0.6, 0.2 + Math.log10(1 + agg.weight) * 0.15);
      const size = Math.min(0.4, 0.08 + Math.log10(1 + agg.weight) * 0.12);
      try {
        graph.addEdgeWithKey(key, String(agg.src), String(agg.tgt), {
          size,
          color: `rgba(${rgb},${alpha})`,
          kindRgb: rgb,
          kind: agg.kind,
        });
      } catch {
        /* duplicate */
      }
    }
    for (const key of graph.edges()) {
      if (!wantedEdges.has(key)) graph.dropEdge(key);
    }
    sigma.refresh({ skipIndexation: true });
  }, [nodes, edges, layoutOverride]);

  useEffect(() => {
    sigmaRef.current?.refresh({ skipIndexation: true });
  }, [focusId, hoverId, visibleKinds, neighbourSet, selected]);

  useEffect(() => {
    if (focusId == null) return;
    const sigma = sigmaRef.current;
    const graph = graphRef.current;
    if (!sigma || !graph) return;
    const idStr = String(focusId);
    if (!graph.hasNode(idStr)) return;
    const attrs = graph.getNodeAttributes(idStr);
    sigma.getCamera().animate(
      { x: attrs.x as number, y: attrs.y as number, ratio: 0.35 },
      { duration: 320 },
    );
  }, [focusId]);

  const doFit = useCallback(() => {
    const sigma = sigmaRef.current;
    const graph = graphRef.current;
    const el = containerRef.current;
    if (!sigma || !graph || !el || graph.order === 0) return;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    graph.forEachNode((_id, attrs) => {
      const x = attrs.x as number;
      const y = attrs.y as number;
      if (x < minX) minX = x; if (x > maxX) maxX = x;
      if (y < minY) minY = y; if (y > maxY) maxY = y;
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
    if (registerFit) registerFit(doFit);
  }, [registerFit, doFit]);

  return (
    <div className="absolute inset-0 overflow-hidden bg-[var(--ct-bg)]">
      <div ref={containerRef} className="absolute inset-0" />
    </div>
  );
}

