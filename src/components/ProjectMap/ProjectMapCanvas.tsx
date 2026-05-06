import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import EdgeCurveProgram from "@sigma/edge-curve";
import { colorForKind, edgeRgbFor } from "./styles";
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
}

const NODE_BASE_SIZE = 3.5;
const FOCUS_NEIGHBOUR_OPACITY = 1.0;
const DIM_OPACITY = 0.05;

function nodeSize(kind: string): number {
  if (kind === "file" || kind === "module") return NODE_BASE_SIZE + 2.5;
  if (kind === "class") return NODE_BASE_SIZE + 1.5;
  return NODE_BASE_SIZE;
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
        if (!visible) {
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
        if (!inFocus) {
          return { ...data, color: dimColor(data.color as string), label: "" };
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
        const [s, t] = graph.extremities(edge);
        const sId = Number(s);
        const tId = Number(t);
        const inFocus =
          neighbourRef.current === null ||
          (neighbourRef.current.has(sId) && neighbourRef.current.has(tId));
        if (!inFocus) {
          return { ...data, color: `rgba(${data.kindRgb ?? "120,128,140"},0.04)` };
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

    const wantedNodes = new Set<string>();
    const fileBuckets = new Map<string, number>();
    for (const n of nodes) {
      const idStr = String(n.id);
      wantedNodes.add(idStr);
      const seed = deterministicSeed(n.id);
      const path = n.path ?? n.serialized_name ?? "";
      let bucket = fileBuckets.get(path);
      if (bucket === undefined) {
        bucket = fileBuckets.size;
        fileBuckets.set(path, bucket);
      }
      const angle = (bucket * 2.39996323) % (Math.PI * 2);
      const radius = 6 + Math.sqrt(bucket + 1) * 1.5;
      const cx = Math.cos(angle) * radius;
      const cy = Math.sin(angle) * radius;
      const x = cx + (seed.x - 0.5) * 1.2;
      const y = cy + (seed.y - 0.5) * 1.2;
      const attrs = {
        x,
        y,
        size: nodeSize(n.kind),
        color: colorForKind(n.kind),
        label: n.display_name,
        kind: n.kind,
      };
      if (graph.hasNode(idStr)) graph.mergeNodeAttributes(idStr, attrs);
      else graph.addNode(idStr, attrs);
    }
    for (const id of graph.nodes()) {
      if (!wantedNodes.has(id)) graph.dropNode(id);
    }

    const wantedEdges = new Set<string>();
    for (const e of edges) {
      const key = `${e.source}->${e.target}-${e.kind}`;
      const sId = String(e.source);
      const tId = String(e.target);
      if (!graph.hasNode(sId) || !graph.hasNode(tId)) continue;
      wantedEdges.add(key);
      if (graph.hasEdge(key)) continue;
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
    for (const key of graph.edges()) {
      if (!wantedEdges.has(key)) graph.dropEdge(key);
    }
    sigma.refresh({ skipIndexation: true });
  }, [nodes, edges]);

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

function dimColor(color: string): string {
  if (color.startsWith("#") && color.length === 7) {
    return `rgba(${parseInt(color.slice(1, 3), 16)},${parseInt(color.slice(3, 5), 16)},${parseInt(
      color.slice(5, 7),
      16,
    )},${DIM_OPACITY})`;
  }
  if (color.startsWith("rgba(")) {
    return color.replace(/,([\d.]+)\)$/, `,${DIM_OPACITY})`);
  }
  return `rgba(120,128,140,${FOCUS_NEIGHBOUR_OPACITY * 0.05})`;
}
