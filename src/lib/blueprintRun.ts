export interface RunEvent {
  decidedBy?: "adapter" | "pattern" | null;
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
  /** The stage's exact automation key and the handler that ran, for drawing the path on typed nodes. */
  stage?: string;
  transition?: string;
  handler?: string | null;
}

export interface GraphRun {
  conversation?: ConversationCounts;
  key: string;
  at: number;
  name: string;
  events: RunEvent[];
  state: "running" | "completed" | "unconfirmed" | "unlinked";
  incomplete: boolean;
}

export interface ConversationCounts {
  enabled: boolean;
  exchanges: number;
  actions: number;
  safety_limit: number;
  rate_limit_per_minute: number;
}

export interface BlueprintRunPopup {
  workspaceId: string;
  teamId: string | null;
  scenarioId: string;
  run: GraphRun;
  count: number;
}
