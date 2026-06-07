// Snapshot graph types — mirror src-tauri/src/mem/graph_layout/snapshot.rs serde shapes.

export interface SnapshotNode {
  id: string;
  x: number;
  y: number;
  kind: string;
  importance: number;
  cluster_id: number;
  has_vector: boolean;
  pinned: boolean;
}

export interface ClusterAgg {
  id: number;
  x: number;
  y: number;
  size: number;
}

export interface ClusterEdge {
  from: number;
  to: number;
  weight: number;
}

export interface SnapshotStats {
  node_count: number;
  cluster_count: number;
  edge_count: number;
  vectorized_without_position: number;
}

export interface GraphSnapshot {
  layout_version: number;
  computed_at: string;
  seq: number;
  nodes: SnapshotNode[];
  clusters: ClusterAgg[];
  cluster_edges: ClusterEdge[];
  stats: SnapshotStats;
}

export interface DeltaNode {
  id: string;
  x: number;
  y: number;
  cluster_id: number;
}

export interface DeltaUnchanged {
  layout_version: number;
  unchanged: true;
}

export interface DeltaChanged {
  layout_version: number;
  seq: number;
  added_or_moved: DeltaNode[];
  removed: string[];
}

export type DeltaResponse = DeltaUnchanged | DeltaChanged | GraphSnapshot;

export interface LabelRow {
  id: string;
  title: string;
}

export interface SnapshotEdge {
  from: string;
  to: string;
  rel: string;
  confidence: number;
}

export function isUnchanged(d: DeltaResponse): d is DeltaUnchanged {
  return (d as DeltaUnchanged).unchanged === true;
}

export function isFullSnapshot(d: DeltaResponse): d is GraphSnapshot {
  return Array.isArray((d as GraphSnapshot).nodes);
}
