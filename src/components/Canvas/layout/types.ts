import type { OrchestratorRef } from "../types";
import type { Box } from "./geometry";

// OPEN by design (§6.3): kinds grow (note/decision/html/prototype/…) without
// editing core. The structured model emits the first three; free-form
// CanvasElement kinds flow through the same render registry.
export type CanvasNodeKind = string;

// Known kinds the structured layout emits — hints only, not a closed union.
export const KNOWN_NODE_KINDS = ["plan", "section", "task", "milestone"] as const;

export interface LaidOutNode {
  id: string;
  kind: CanvasNodeKind;
  planId: string;
  box: Box;
  iteration: number | null;
  manual: boolean;
}

export type CanvasEdgeKind = "section-task" | "milestone-chain";

export interface LaidOutEdge {
  id: string;
  from: string;
  to: string;
  kind: CanvasEdgeKind;
}

export interface PlanFrame {
  planId: string;
  title: string;
  owner: OrchestratorRef;
  ownerKey: string;
  box: Box;
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
}

export interface WorkspaceLayout {
  frames: PlanFrame[];
  nodeIndex: Map<string, LaidOutNode>;
  bounds: Box;
}

// Frame in local coords (origin 0,0), before region packing offsets it.
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
}
