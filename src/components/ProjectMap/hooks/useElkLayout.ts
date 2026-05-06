import { useEffect, useState } from "react";
import ELK from "elkjs/lib/elk.bundled.js";
import type { ProjectMapEdge, ProjectMapNode } from "../types";

const elk = new ELK();

export interface ElkPosition {
  id: number;
  x: number;
  y: number;
}

export interface ElkResult {
  positions: Map<number, ElkPosition>;
  loading: boolean;
  error: string | null;
}

const DSM_KINDS = new Set(["file", "module"]);
const NODE_W = 60;
const NODE_H = 40;

export function useElkLayout(
  nodes: ProjectMapNode[],
  edges: ProjectMapEdge[],
  enabled: boolean,
): ElkResult {
  const [positions, setPositions] = useState<Map<number, ElkPosition>>(new Map());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) {
      setPositions(new Map());
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);

    const fileNodes = nodes.filter((n) => DSM_KINDS.has(n.kind));
    const fileIds = new Set(fileNodes.map((n) => n.id));

    const aggregated = aggregateEdges(edges, fileIds);
    const elkGraph = {
      id: "root",
      layoutOptions: {
        "elk.algorithm": "layered",
        "elk.direction": "DOWN",
        "elk.layered.spacing.nodeNodeBetweenLayers": "60",
        "elk.spacing.nodeNode": "30",
        "elk.layered.crossingMinimization.semiInteractive": "true",
      },
      children: fileNodes.map((n) => ({ id: String(n.id), width: NODE_W, height: NODE_H })),
      edges: aggregated.map((e, i) => ({
        id: `e${i}`,
        sources: [String(e.source)],
        targets: [String(e.target)],
      })),
    };

    elk
      .layout(elkGraph)
      .then((laid) => {
        if (cancelled) return;
        const next = new Map<number, ElkPosition>();
        const rawNodes = laid.children ?? [];
        let maxX = 1;
        let maxY = 1;
        for (const c of rawNodes) {
          const x = (c.x ?? 0) + (c.width ?? NODE_W) / 2;
          const y = (c.y ?? 0) + (c.height ?? NODE_H) / 2;
          if (x > maxX) maxX = x;
          if (y > maxY) maxY = y;
        }
        const scale = 30 / Math.max(maxX, maxY);
        for (const c of rawNodes) {
          const id = Number(c.id);
          const x = ((c.x ?? 0) + (c.width ?? NODE_W) / 2) * scale - 15;
          const y = ((c.y ?? 0) + (c.height ?? NODE_H) / 2) * scale - 15;
          next.set(id, { id, x, y });
        }
        setPositions(next);
        setLoading(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [enabled, nodes, edges]);

  return { positions, loading, error };
}

function aggregateEdges(
  edges: ProjectMapEdge[],
  fileIds: Set<number>,
): { source: number; target: number; weight: number }[] {
  const counts = new Map<string, { source: number; target: number; weight: number }>();
  for (const e of edges) {
    if (!fileIds.has(e.source) || !fileIds.has(e.target)) continue;
    if (e.source === e.target) continue;
    if (e.kind === "contain") continue;
    const key = `${e.source}->${e.target}`;
    const existing = counts.get(key);
    if (existing) {
      existing.weight += e.weight;
    } else {
      counts.set(key, { source: e.source, target: e.target, weight: e.weight });
    }
  }
  return Array.from(counts.values());
}
