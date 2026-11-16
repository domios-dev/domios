import type { SnapshotEdge } from "./snapshotTypes";

export function edgeKey(edge: SnapshotEdge): string {
  return JSON.stringify([edge.from, edge.to, edge.rel]);
}

export function restEdgeStrength(ratio: number): number | null {
  if (ratio > 0.7) return null;
  if (ratio >= 0.4) return 0.1;
  if (ratio >= 0.15) return 0.14;
  return 0.18;
}

export function longEdgeFactor(length: number, width: number, height: number): number {
  return length > 0.35 * Math.hypot(width, height) ? 0.5 : 1;
}

export function mutualTopEdges(input: Iterable<SnapshotEdge>): SnapshotEdge[] {
  const unique = new Map<string, SnapshotEdge>();
  for (const edge of input) unique.set(edgeKey(edge), edge);
  const ranked = [...unique.values()].sort((a, b) => (b.confidence ?? 0.5) - (a.confidence ?? 0.5)
    || compare(a.rel, b.rel) || compare(edgeKey(a), edgeKey(b)));
  const byNode = new Map<string, SnapshotEdge[]>();
  for (const edge of ranked) {
    for (const id of new Set([edge.from, edge.to])) {
      const list = byNode.get(id) ?? [];
      list.push(edge);
      byNode.set(id, list);
    }
  }
  const top = new Map([...byNode].map(([id, edges]) => [id, new Set(edges.slice(0, 3).map(edgeKey))]));
  const retained = new Set(ranked.filter(edge => top.get(edge.from)?.has(edgeKey(edge))
    && top.get(edge.to)?.has(edgeKey(edge))).map(edgeKey));
  const connected = new Set(ranked.filter(edge => retained.has(edgeKey(edge))).flatMap(edge => [edge.from, edge.to]));
  for (const [id, edges] of byNode) {
    if (!connected.has(id)) retained.add(edgeKey(edges[0]));
  }
  return ranked.filter(edge => retained.has(edgeKey(edge)));
}

function compare(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export class SettledEdgeUpdates {
  private moving = false;

  beginGesture() { this.moving = true; }

  update(apply: () => void): boolean {
    if (this.moving) return false;
    apply();
    return true;
  }

  settle(apply: () => void) {
    this.moving = false;
    apply();
  }
}
