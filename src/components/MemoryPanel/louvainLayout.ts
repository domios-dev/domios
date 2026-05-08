import type Graph from "graphology";
import louvain from "graphology-communities-louvain";

const CENTROID_RADIUS_FACTOR = 80;
const INNER_RADIUS_FACTOR = 60;

export function layoutLouvain(graph: Graph): void {
  if (graph.order === 0) return;

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
