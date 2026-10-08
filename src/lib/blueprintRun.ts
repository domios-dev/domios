export interface RunEvent {
  classifier?: { ai: boolean; label: string; detail?: string } | null;
  confidence?: number | null;
  reason?: string | null;
  at: number;
  node: string;
  outcome: string;
  target: string | null;
  status: string;
  duration: number | null;
  edgeId: string | null;
}

export interface GraphRun {
  key: string;
  at: number;
  name: string;
  events: RunEvent[];
  state: "running" | "completed" | "unconfirmed" | "unlinked";
  incomplete: boolean;
}

export interface BlueprintRunPopup {
  workspaceId: string;
  teamId: string | null;
  scenarioId: string;
  run: GraphRun;
  count: number;
}
