import type { OrchestratorRef } from "../types";
import type { Box, Point } from "./geometry";

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

// point (section-row anchors and cluster anchors that are not standalone nodes).
export interface LaidOutEdge {
  id: string;
  kind: CanvasEdgeKind;
  from?: string;
  to?: string;
  fromPoint?: Point;
  toPoint?: Point;
}

export interface SectionZone {
  sectionId: string;
  label: string | null;
  color: string;
  hull: Point[];
  labelAnchor: Point;
  box?: Box;
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
  ownerTabId: string | null;
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
