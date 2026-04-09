import { memo, useEffect, useRef } from "react";
import Sigma from "sigma";
import Graph from "graphology";
import type { MemEdge, MemNode } from "./types";
import { colorForKind } from "./nodeKind";

interface Props {
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  viewport: { panX: number; panY: number; scale: number };
  onNodeClick: (id: string) => void;
}

function MemoryCanvasOverviewImpl({ nodes, edges, viewport, onNodeClick }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const initedRef = useRef(false);

  useEffect(() => {
    if (initedRef.current) return;
    initedRef.current = true;
    if (!containerRef.current) return;
    try {
      const graph = new Graph();
      graphRef.current = graph;
      const sigma = new Sigma(graph, containerRef.current, {
        renderLabels: false,
        renderEdgeLabels: false,
        defaultNodeType: "circle",
        defaultEdgeType: "line",
      });
      sigmaRef.current = sigma;
      sigma.on("clickNode", ({ node }) => onNodeClick(node));
    } catch (err) {
      console.warn("[MemoryCanvasOverview] sigma init failed", err);
    }
    return () => {
      sigmaRef.current?.kill();
      sigmaRef.current = null;
      graphRef.current = null;
      initedRef.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma) return;
    graph.clear();
    for (const n of nodes.values()) {
      if (n.graph_x == null || n.graph_y == null) continue;
      graph.addNode(n.id, {
        x: n.graph_x,
        y: -n.graph_y,
        size: Math.max(3, Math.min(10, 3 + (n.importance ?? 3) * 0.7)),
        color: colorForKind(n.kind),
        label: n.title,
      });
    }
    for (const e of edges) {
      if (!graph.hasNode(e.from) || !graph.hasNode(e.to)) continue;
      try {
        graph.addEdge(e.from, e.to, {
          size: 0.5 + (e.confidence ?? 0.5) * 0.8,
          color: `rgba(255,255,255,${0.15 + (e.confidence ?? 0.5) * 0.4})`,
        });
      } catch {
        /* duplicate edge */
      }
    }
    sigma.refresh();
  }, [nodes, edges]);

  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;
    try {
      sigma.getCamera().setState({
        ratio: 1 / Math.max(viewport.scale, 0.01),
      });
    } catch {
      /* ignore */
    }
  }, [viewport]);

  return <div ref={containerRef} style={{ width: "100%", height: "100%" }} />;
}

export default memo(MemoryCanvasOverviewImpl);
