import type { OrchestratorRef } from "../types";
import type { Box, Point } from "./geometry";

// OPEN by design: kinds grow (spec/orchestrator/note/html/…) without editing
// core. The structured model emits the known kinds below; free-form
// CanvasElement kinds flow through the same render registry.
export type CanvasNodeKind = string;

export const KNOWN_NODE_KINDS = [
  "spec",
  "plan",
  "task",
] as const;

export interface LaidOutNode {
  id: string;
  kind: CanvasNodeKind;
  planId: string;
  box: Box;
  iteration: number | null;
  manual: boolean;
}

export type CanvasEdgeKind =
  | "section-task"
  | "spec-plan"
  | "task-cluster"
  | "milestone-chain"
  | "subtask"
  | "plan-spec"
  | "plan-plan"
  | "handoff-chain";

// An edge endpoint is either a live node (follows drag) or a static world/local
// point (section-row anchors and cluster anchors that are not standalone nodes).
export interface LaidOutEdge {
  id: string;
  kind: CanvasEdgeKind;
  from?: string;
  to?: string;
  fromPoint?: Point;
  toPoint?: Point;
}

// A section TERRITORY: the organic enclosing region around one section's task
// cluster. `hull` is the inflated boundary polygon (world coords) the overlay
// smooths into a blob; `label` names it (null = the neutral unsectioned group).
export interface SectionZone {
  sectionId: string;
  label: string | null;
  color: string;
  hull: Point[];
  labelAnchor: Point;
}

export interface PlanFrame {
  planId: string;
  title: string;
  owner: OrchestratorRef;
  ownerKey: string;
  box: Box;
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  // Per-section-row right-edge anchors in WORLD coords; edge source for tasks.
  anchors: Map<string, Point>;
  // Section territories (organic hulls around each section's task cluster).
  zones: SectionZone[];
}

// Soft labeled wrapper around all plans owned by one orchestrator (§2.5).
export interface OwnerGroup {
  ownerKey: string;
  title: string;
  ownerPaneId: string | null;
  box: Box;
  planIds: string[];
}

export interface WorkspaceLayout {
  frames: PlanFrame[];
  nodeIndex: Map<string, LaidOutNode>;
  groups: OwnerGroup[];
  bounds: Box;
  crossEdges?: LaidOutEdge[];
}

// Frame in local coords (origin 0,0), before region packing offsets it. Points
// in edges/anchors are local too and get translated alongside node boxes.
export interface IntraPlanLayout {
  planId: string;
  title: string;
  owner: OrchestratorRef;
  ownerKey: string;
  updatedAt: number;
  width: number;
  height: number;
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  anchors: Map<string, Point>;
  zones: SectionZone[];
}
