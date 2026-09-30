export interface SwarmNode {
  id: string;          // tab_id
  title: string;
  role: 'Orchestrator' | 'Worker' | null;
  status: 'Working' | 'Waiting' | 'Idle' | 'Error' | 'Dead';
  parentId: string | null;
  x: number;           // computed position
  y: number;
  targetX: number;     // layout target
  targetY: number;
  spawnTime: number;    // timestamp when node appeared
  deadTime: number | null; // timestamp when node died (null = alive)
  flashColor: string | null; // transient border flash color
  flashStart: number;   // timestamp when flash started
}

export interface SwarmEdge {
  from: string;        // parent tab_id
  to: string;          // child tab_id
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
