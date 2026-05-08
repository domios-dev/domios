export interface PhysicsNode {
  id: string;
  x: number;
  y: number;
}

export interface PhysicsEdge {
  from: string;
  to: string;
  confidence: number | null;
}

export interface PhysicsPosition {
  id: string;
  x: number;
  y: number;
}

export type PhysicsRequest =
  | { type: "setGraph"; nodes: PhysicsNode[]; edges: PhysicsEdge[] }
  | { type: "updatePosition"; id: string; x: number; y: number }
  | { type: "heat" }
  | { type: "setEnabled"; enabled: boolean };

export type PhysicsResponse = { type: "tick"; positions: PhysicsPosition[] };
