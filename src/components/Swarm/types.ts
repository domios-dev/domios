export interface SwarmNode {
  id: string;          // pane_id
  title: string;
  role: 'Orchestrator' | 'Lead' | 'Worker' | null;
  status: 'Working' | 'Waiting' | 'Idle' | 'Dead';
  parentId: string | null;
  x: number;           // computed position
  y: number;
  targetX: number;     // layout target
  targetY: number;
  // Animation state
  spawnTime: number;    // timestamp when node appeared
  deadTime: number | null; // timestamp when node died (null = alive)
  flashColor: string | null; // transient border flash color
  flashStart: number;   // timestamp when flash started
}

export interface SwarmEdge {
  from: string;        // parent pane_id
  to: string;          // child pane_id
  particles: ReportParticle[];
}

export interface ReportParticle {
  id: string;
  progress: number;    // 0 to 1 along the edge
  startTime: number;
}

export interface SwarmState {
  nodes: Map<string, SwarmNode>;
  edges: Map<string, SwarmEdge>;
  viewport: { x: number; y: number; zoom: number };
}
