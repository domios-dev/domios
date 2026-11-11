// Snapshot graph types — mirror src-tauri/src/mem/graph_layout/snapshot.rs serde shapes.

export type SnapshotNode = import("../../../gen/SnapshotNode").SnapshotNode;

export type ClusterAgg = import("../../../gen/ClusterAgg").ClusterAgg;

export type ClusterEdge = import("../../../gen/ClusterEdge").ClusterEdge;

export type SnapshotStats = import("../../../gen/SnapshotStats").SnapshotStats;

export type GraphSnapshot = import("../../../gen/Snapshot").Snapshot;

export type DeltaNode = import("../../../gen/DeltaNode").DeltaNode;

export type DeltaUnchanged = Extract<import("../../../gen/DeltaResponse").DeltaResponse, { unchanged: boolean }> & { unchanged: true };

export type DeltaChanged = Extract<import("../../../gen/DeltaResponse").DeltaResponse, { seq: number }>;

export type DeltaResponse = DeltaUnchanged | DeltaChanged | GraphSnapshot;

export type LabelRow = import("../../../gen/LabelRow").LabelRow;

export type SnapshotEdge = import("../../../gen/EdgeRow").EdgeRow;

export function isUnchanged(d: DeltaResponse): d is DeltaUnchanged {
  return (d as DeltaUnchanged).unchanged === true;
}

export function isFullSnapshot(d: DeltaResponse): d is GraphSnapshot {
  return Array.isArray((d as GraphSnapshot).nodes);
}
