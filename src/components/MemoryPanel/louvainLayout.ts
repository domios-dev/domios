import type Graph from "graphology";
import louvain from "graphology-communities-louvain";
import forceAtlas2 from "graphology-layout-forceatlas2";
import noverlap from "graphology-layout-noverlap";

const CENTROID_RADIUS = 1500;
const MIN_INNER_RADIUS = 80;
const INNER_RADIUS_BASE = 300;

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

  const communities = [...buckets.keys()];
  const k = Math.max(1, communities.length);
  const seed = new Map<string, { x: number; y: number }>();

  communities.forEach((c, i) => {
    const members = buckets.get(c)!;
    const angle = (Math.PI * 2 * i) / k;
    const cx = CENTROID_RADIUS * Math.cos(angle);
    const cy = CENTROID_RADIUS * Math.sin(angle);
    const n = members.length;
    const innerRadius = Math.max(
      MIN_INNER_RADIUS,
      INNER_RADIUS_BASE / Math.sqrt(Math.max(1, n)),
    );
    members.forEach((id, j) => {
      const nodeAngle = (Math.PI * 2 * j) / Math.max(1, n);
      seed.set(id, {
        x: cx + innerRadius * Math.cos(nodeAngle),
        y: cy + innerRadius * Math.sin(nodeAngle),
      });
    });
  });

  graph.updateEachNodeAttributes((id, attrs) => {
    const p = seed.get(id);
    if (!p) return attrs;
    return { ...attrs, x: p.x, y: p.y };
  });

  forceAtlas2.assign(graph, { iterations: 100 });
  noverlap.assign(graph, { maxIterations: 50 });
}
