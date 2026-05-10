import type Graph from "graphology";
import louvain from "graphology-communities-louvain";
import { hashString as hashSeed } from "./seedLayout";

const CENTROID_RADIUS_FACTOR = 80;
const INNER_RADIUS_FACTOR = 60;

export function layoutLouvain(graph: Graph): void {
  if (graph.order === 0) return;
  // Louvain requires a non-mixed graph; inferType returns "mixed" when
  // directedSize === 0 && undirectedSize === 0 (no edges). Skip community
  // detection in that case — every node is its own cluster anyway.
  if (graph.size === 0) {
    graph.forEachNode((id) => {
      graph.setNodeAttribute(id, "community", 0);
    });
    const n = graph.order;
    const ringRadius = CENTROID_RADIUS_FACTOR * Math.sqrt(n);
    graph.forEachNode((id, _attrs) => {
      const h = hashSeed(id);
      const angle = (h / 0xffffffff) * Math.PI * 2;
      const jitter = ((h >>> 16) / 0xffff - 0.5) * ringRadius * 0.3;
      graph.mergeNodeAttributes(id, {
        x: (ringRadius + jitter) * Math.cos(angle),
        y: (ringRadius + jitter) * Math.sin(angle),
      });
    });
    return;
  }

  louvain.assign(graph);

  const buckets = new Map<number, string[]>();
  graph.forEachNode((id, attrs) => {
    const c = (attrs as { community?: number }).community ?? 0;
    let arr = buckets.get(c);
    if (!arr) {
      arr = [];
      buckets.set(c, arr);
    }
    arr.push(id);
  });

  // Big clusters first so they get adjacent ring slots; singletons fill the
  // tail. The physics worker animates from this seed (no fa2/noverlap pass).
  const communities = [...buckets.values()].sort((a, b) => b.length - a.length);
  const k = Math.max(1, communities.length);
  const N = graph.order;
  const centroidRadius = CENTROID_RADIUS_FACTOR * Math.sqrt(N);

  communities.forEach((members, i) => {
    const angle = (Math.PI * 2 * i) / k;
    const cx = centroidRadius * Math.cos(angle);
    const cy = centroidRadius * Math.sin(angle);
    const n = members.length;
    if (n === 1) {
      graph.mergeNodeAttributes(members[0], { x: cx, y: cy });
      return;
    }
    const innerRadius = INNER_RADIUS_FACTOR * Math.sqrt(n);
    members.forEach((id, j) => {
      const nodeAngle = (Math.PI * 2 * j) / n;
      graph.mergeNodeAttributes(id, {
        x: cx + innerRadius * Math.cos(nodeAngle),
        y: cy + innerRadius * Math.sin(nodeAngle),
      });
    });
  });
}
