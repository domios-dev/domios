// TS mirror of the Rust plan model (src-tauri/src/plan/types.rs). Field names
// match the JSON wire shape (serde snake_case; Task.type via serde rename).

export type PlanStatus = "active" | "archived";
export type TaskStatus = "pending" | "in_progress" | "in_review" | "blocked" | "done" | "cancelled";
export type LoopStatus = "running" | "done" | "exhausted" | "stopped";
export type DriftKind = "in_progress_pane_gone" | "stale_in_review" | "bound_to_closed_pane" | "accepted_but_not_done";

export interface DriftItem {
  task_id: string;
  plan_id: string;
  kind: DriftKind;
  detail: string;
  suggested_fix: string;
  not_mine: boolean;
}

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

export interface TaskLaneMetadata {
  pane_id?: string | null;
  workspace_id?: string | null;
  workspace_name?: string | null;
  worktree_path?: string | null;
  branch?: string | null;
  base?: string | null;
  destination?: string | null;
  cleanup_status?: string | null;
  cleanup_hints?: string[];
}

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

export interface TagFacetValue {
  v: string;
  label?: string | null;
  color?: string | null;
}

export interface TagFacet {
  key: string;
  label?: string | null;
  color_lane?: boolean;
  values: readonly TagFacetValue[];
}

export interface Section {
  id: string;
  title: string;
  body: string;
  tags?: string[];
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

export type TaskPriority = "p0" | "p1" | "p2" | "p3"; // p0 = highest

export interface Task {
  id: string;
  name: string;
  type: string; // open string: implement | research | review | ...
  model: string | null;
  model_rationale: string | null;
  status: TaskStatus;
  status_label: string | null;
  priority?: TaskPriority;
  created_at?: number;
  updated_at?: number;
  status_changed_at?: number;
  handle?: string | null;
  tags?: string[];
  group?: string | null;
  section_ids: string[];
  body: string;
  acceptance_criteria?: string[];
  test_plan?: string;
  intent: string | null;
  parent_task_id: string | null;
  assigned_pane_id: string | null;
  lane?: TaskLaneMetadata;
  taken_by_role?: string | null;
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

export interface Goal {
  vector: string;
  vision?: string;
  constraints?: string[];
  postulates?: string[];
}

export interface HeartbeatLearning {
  id: string;
  text: string;
  ts: number;
  tag?: string | null;
}

export interface OrchestratorHeartbeat {
  current_focus?: string | null;
  active_lanes?: string[];
  rules?: string[];
  learnings?: HeartbeatLearning[];
}

export interface SpecLink {
  spec_id: string;
  spec_section_id: string;
  plan_section_id: string;
}

export type EffortLinkRel = "follows" | "supersedes";

export interface EffortLink {
  to: string;
  rel: EffortLinkRel;
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
  tag_facets?: TagFacet[];
  loops: LoopSpec[];
  elements: CanvasElement[];
  tasks: Task[];
  canvas: PlanCanvas;
  owner_history: TransferRecord[];
  goal?: Goal | null;
  heartbeat?: OrchestratorHeartbeat;
  primary_spec_id?: string | null;
  spec_links?: SpecLink[];
  links?: string[];
  lineage?: EffortLink[];
}
