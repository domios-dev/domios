// TS mirror of the Rust plan model (src-tauri/src/plan/types.rs). Field names
// match the JSON wire shape (serde snake_case; Task.type via serde rename).

export type PlanStatus = "active" | "archived";
export type TaskStatus = "pending" | "in_progress" | "in_review" | "blocked" | "paused" | "done" | "cancelled";
// instead of bare string literals (type + value namespaces merge by design).
export const TaskStatus = {
  Pending: "pending",
  InProgress: "in_progress",
  InReview: "in_review",
  Blocked: "blocked",
  Paused: "paused",
  Done: "done",
  Cancelled: "cancelled",
} as const satisfies Record<string, TaskStatus>;
export type LoopStatus = "running" | "done" | "exhausted" | "stopped";
export type DriftKind =
  | "in_progress_tab_gone"
  | "stale_in_review"
  | "stale_pending_unstaffed"
  | "bound_to_closed_tab"
  | "accepted_but_not_done";

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
  tab_id: string | null;
  session_id: string | null;
  title: string | null;
}
export type AgentRef = OrchestratorRef;

export type PlannerRef =
  | { kind: "tab"; tab_id: string; session_id?: string | null; title?: string | null }
  | { kind: "waived"; note: string };

export type PlanMode = "planning" | "approved" | "execution" | "paused" | "complete";
export type PlanningState = "drafting" | "structure_ready" | "approved";

export interface TaskLaneMetadata {
  tab_id?: string | null;
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

export interface PlanLifecycleEvent {
  id: string;
  kind: string;
  at: number;
  author: OrchestratorRef;
}

export interface MilestoneKind {
  key: string;
  label: string;
  color: string | null;
}

export interface TaskGroup {
  key: string;
  label: string;
  color?: string | null;
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
  attachments?: PlanAttachment[];
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
  attachments?: PlanAttachment[];
}

export interface PlanAttachment {
  id: string;
  name: string;
  path: string;
  media_type: string;
  size_bytes: number;
  created_at: number;
}

export type TaskPriority = "p0" | "p1" | "p2" | "p3"; // p0 = highest
export const TaskPriority = { P0: "p0", P1: "p1", P2: "p2", P3: "p3" } as const satisfies Record<string, TaskPriority>;
export type ReviewStage = "review" | "qa";
export type QaState = "queued" | "running";

export interface TaskCompetitionMetadata {
  enabled?: boolean;
  attempts?: number | null;
  models?: string[];
  judge_hint?: string | null;
}

export interface TaskExecutionReportSnapshot {
  id?: string;
  message?: string | null;
  status?: string | null;
}

export interface TaskExecutionAttempt {
  id?: string;
  tab_id?: string | null;
  tab_title?: string | null;
  executor_tab_id?: string | null;
  executor_title?: string | null;
  provider?: string | null;
  agent_type?: string | null;
  model?: string | null;
  reasoning_effort?: string | null;
  session_model?: string | null;
  session_id?: string | null;
  role?: string | null;
  branch?: string | null;
  worktree_path?: string | null;
  report?: TaskExecutionReportSnapshot | null;
  report_status?: string | null;
  assigned_at?: number;
  reported_at?: number;
  closed_at?: number;
  outcome?: string | null;
  competition_group_id?: string | null;
  competition_lane?: number | null;
  judge_verdict?: string | null;
  judge_reason?: string | null;
  judged_at?: number;
  winner?: boolean;
}

export interface ExternalRef {
  kind: "github_issue";
  repository: string;
  number: number;
  canonical_url: string;
  relationship?: "fixes" | "relates_to";
}

export interface Task {
  id: string;
  name: string;
  type: string; // open string: implement | research | review | ...
  model: string | null;
  model_rationale: string | null;
  status: TaskStatus;
  review_stage?: ReviewStage;
  qa_state?: QaState | null;
  status_label: string | null;
  priority?: TaskPriority;
  created_at?: number;
  updated_at?: number;
  status_changed_at?: number;
  handle?: string | null;
  /** Immutable human key (`PREFIX-N`); absent on tasks created before keys existed. */
  key?: string | null;
  tags?: string[];
  group?: string | null;
  section_ids: string[];
  body: string;
  role_guidance?: string | null;
  executor_role?: string | null;
  personality?: string | null;
  acceptance_criteria?: string[];
  external_refs?: ExternalRef[];
  test_plan?: string;
  /** Correctness vector — what "done right" means (rendered in the READ detail). */
  correctness?: string;
  /** Task-level reference links (rendered in the READ detail). */
  links?: string[];
  intent: string | null;
  parent_task_id: string | null;
  assigned_tab_id: string | null;
  lane?: TaskLaneMetadata;
  taken_by_role?: string | null;
  peer: string | null;
  competition?: TaskCompetitionMetadata | null;
  execution_attempts?: TaskExecutionAttempt[];
  pause_reason?: string | null;
  canvas: ElementCanvas;
  milestones: Milestone[];
  attachments?: PlanAttachment[];
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
  stages?: LoopStage[];
  budget: number;
  count: number;
  stop_when: string | null;
  status: LoopStatus;
  stagnation?: number;
  deviation?: string | null;
  last_iteration_at?: number | null;
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
  mode?: PlanMode;
  planning_state?: PlanningState;
  owner: OrchestratorRef;
  planner?: PlannerRef | null;
  /** Optional 1:1 team binding; absent keeps legacy plans teamless. */
  team_id?: string | null;
  project_path: string;
  created_at: number;
  updated_at: number;
  milestone_kinds: MilestoneKind[];
  task_groups?: TaskGroup[];
  tag_facets?: TagFacet[];
  loops: LoopSpec[];
  elements: CanvasElement[];
  tasks: Task[];
  canvas: PlanCanvas;
  owner_history: TransferRecord[];
  lifecycle_events?: PlanLifecycleEvent[];
  goal?: Goal | null;
  heartbeat?: OrchestratorHeartbeat;
  primary_spec_id?: string | null;
  spec_links?: SpecLink[];
  links?: string[];
  lineage?: EffortLink[];
  strides?: Milestone[];
  gates?: DecisionGate[];
}

export type GateStatus = "open" | "answered" | "escalated";

export interface GateAnswer {
  text: string;
  by_tab_id?: string | null;
  at: number;
}

/** Structured decision request that pauses blocking tasks until answered. */
export interface DecisionGate {
  id: string;
  question: string;
  context?: string;
  blocking_task_ids?: string[];
  routed_to_tab_id?: string | null;
  asked_by_tab_id?: string | null;
  asked_at: number;
  status: GateStatus;
  answer?: GateAnswer | null;
  escalated_at?: number | null;
  escalation_note?: string | null;
}
