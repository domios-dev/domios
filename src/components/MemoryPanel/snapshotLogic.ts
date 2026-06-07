// Pure, testable logic for the snapshot graph: delta merge + LOD decisions.

import type { DeltaChanged, GraphSnapshot, SnapshotNode } from "./snapshotTypes";

// Camera ratio above this threshold = zoomed out → cluster-aggregate view.
export const LOD_CLUSTER_RATIO = 1.5;
// Min importance rank kept as item nodes while zoomed out (overview anchors).
export const LOD_OVERVIEW_IMPORTANCE = 4;
// Cap on labels requested per /labels batch (matches backend MAX_IDS).
export const LABEL_BATCH_CAP = 500;
// Cap on ids per /edges request (matches backend MAX_IDS).
export const EDGE_BATCH_CAP = 500;
// Min importance for a viewport node to qualify for a lazy label fetch.
export const LABEL_MIN_IMPORTANCE = 2;

export interface NodeIndex {
  byId: Map<string, SnapshotNode>;
  clusters: GraphSnapshot["clusters"];
  clusterEdges: GraphSnapshot["cluster_edges"];
  layoutVersion: number;
  seq: number;
  computedAt: string;
}

export function indexSnapshot(snap: GraphSnapshot): NodeIndex {
  const byId = new Map<string, SnapshotNode>();
  for (const n of snap.nodes) byId.set(n.id, n);
  return {
    byId,
    clusters: snap.clusters,
    clusterEdges: snap.cluster_edges,
    layoutVersion: snap.layout_version,
    seq: snap.seq,
    computedAt: snap.computed_at,
  };
}

// Apply an in-version delta onto an existing index, returning a new index.
export function applyDelta(prev: NodeIndex, delta: DeltaChanged): NodeIndex {
  const byId = new Map(prev.byId);
  for (const id of delta.removed) byId.delete(id);
  for (const d of delta.added_or_moved) {
    const existing = byId.get(d.id);
    if (existing) {
      byId.set(d.id, { ...existing, x: d.x, y: d.y, cluster_id: d.cluster_id });
    } else {
      // New node from a peer save before we have its meta — placeholder kind.
      byId.set(d.id, {
        id: d.id,
        x: d.x,
        y: d.y,
        cluster_id: d.cluster_id,
        kind: "note",
        importance: 0,
        has_vector: true,
        pinned: false,
      });
    }
  }
  return { ...prev, byId, layoutVersion: delta.layout_version, seq: delta.seq };
}

export function isZoomedOut(cameraRatio: number): boolean {
  return cameraRatio > LOD_CLUSTER_RATIO;
}

// Item nodes kept while zoomed out: only the top-importance anchors.
export function overviewNodes(nodes: SnapshotNode[]): SnapshotNode[] {
  return nodes.filter((n) => n.importance >= LOD_OVERVIEW_IMPORTANCE);
}

export interface Bounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

export function inViewport(n: SnapshotNode, b: Bounds): boolean {
  return n.x >= b.minX && n.x <= b.maxX && n.y >= b.minY && n.y <= b.maxY;
}

// Viewport nodes that deserve a lazy label fetch, highest importance first, capped.
export function labelCandidates(
  nodes: SnapshotNode[],
  bounds: Bounds,
  alreadyFetched: Set<string>,
  cap: number = LABEL_BATCH_CAP,
): string[] {
  const out = nodes
    .filter(
      (n) =>
        n.importance >= LABEL_MIN_IMPORTANCE &&
        !alreadyFetched.has(n.id) &&
        inViewport(n, bounds),
    )
    .sort((a, b) => b.importance - a.importance)
    .slice(0, cap)
    .map((n) => n.id);
  return out;
}

// Viewport node ids that need their real edges, capped, skipping cached ones.
export function edgeCandidates(
  nodes: SnapshotNode[],
  bounds: Bounds,
  alreadyFetched: Set<string>,
  cap: number = EDGE_BATCH_CAP,
): string[] {
  const out: string[] = [];
  for (const n of nodes) {
    if (out.length >= cap) break;
    if (alreadyFetched.has(n.id)) continue;
    if (inViewport(n, bounds)) out.push(n.id);
  }
  return out;
}
