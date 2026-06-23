// TS mirror of the Rust plan model (src-tauri/src/plan/types.rs). Field names
// match the JSON wire shape (serde snake_case; Task.type via serde rename).

export type PlanStatus = "active" | "archived";
export type TaskStatus = "pending" | "in_progress" | "blocked" | "done" | "cancelled";
export type LoopStatus = "running" | "done" | "exhausted" | "stopped";

export interface ElementCanvas {
  x: number | null;
  y: number | null;
  collapsed: boolean;
}

export interface OrchestratorRef {
  pane_id: string | null;
  session_id: string | null;
  title: string | null;
}
export type AgentRef = OrchestratorRef;

export interface TransferRecord {
  from: OrchestratorRef;
  to: OrchestratorRef;
  ts: number;
  reason: string;
}

export interface MilestoneKind {
  key: string;
  label: string;
  color: string | null;
}

export interface Section {
  id: string;
  title: string;
  body: string;
  subsections: Section[];
  canvas: ElementCanvas;
}

export interface Milestone {
  id: string;
  from: AgentRef;
  kind: string;
  text: string;
  ts: number;
  links: string[];
  loop_id: string | null;
  iteration: number | null;
  canvas: ElementCanvas;
}

export interface Task {
  id: string;
  name: string;
  type: string; // open string: implement | research | review | ...
  model: string | null;
  model_rationale: string | null;
  status: TaskStatus;
  status_label: string | null;
  section_ids: string[];
  body: string;
  intent: string | null;
  assigned_pane_id: string | null;
  peer: string | null;
  canvas: ElementCanvas;
  milestones: Milestone[];
}

export interface LoopStage {
  key: string;
  role: string;
  model: string | null;
  why: string | null;
  prompt: string | null;
}

export interface LoopSpec {
  id: string;
  target: string;
  stages: LoopStage[];
  budget: number;
  count: number;
  stop_when: string | null;
  status: LoopStatus;
}

export interface CanvasElement {
  id: string;
  kind: string;
  canvas: ElementCanvas;
  content: unknown;
  links: string[];
}

export interface Viewport {
  pan_x: number;
  pan_y: number;
  scale: number;
}

export interface PlanCanvas {
  viewport: Viewport | null;
}

export interface Plan {
  id: string;
  title: string;
  sections: Section[];
  status: PlanStatus;
  owner: OrchestratorRef;
  project_path: string;
  created_at: number;
  updated_at: number;
  milestone_kinds: MilestoneKind[];
  loops: LoopSpec[];
  elements: CanvasElement[];
  tasks: Task[];
  canvas: PlanCanvas;
  owner_history: TransferRecord[];
}
