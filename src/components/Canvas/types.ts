type PlanHydrated<T, K extends keyof T, N extends keyof T = never> = Pick<{ [P in keyof T]: T[P] | (P extends N ? null : never) }, K> & Partial<Omit<{ [P in keyof T]: T[P] | (P extends N ? null : never) }, K>>;
type NullableOptionals<T> = T extends unknown ? { [K in keyof T]: T[K] | ({} extends Pick<T, K> ? null : never) } : never;

// Canvas hydration views compose the generated Rust wire contracts.

export type PlanStatus = import("../../../gen/PlanStatus").PlanStatus;
export type TaskStatus = import("../../../gen/TaskStatus").TaskStatus;
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
export type LoopStatus = import("../../../gen/LoopStatus").LoopStatus;
export type DriftKind = import("../../../gen/DriftKind").DriftKind;

export type DriftItem = Omit<PlanHydrated<Pick<import("../../../gen/DriftItem").DriftItem, Extract<keyof import("../../../gen/DriftItem").DriftItem, "task_id" | "plan_id" | "kind" | "detail" | "suggested_fix" | "not_mine">>, "task_id" | "plan_id" | "kind" | "detail" | "suggested_fix" | "not_mine", never>, "kind"> & {
  kind: DriftKind;
};

export type ElementCanvas = PlanHydrated<Pick<import("../../../gen/ElementCanvas").ElementCanvas, Extract<keyof import("../../../gen/ElementCanvas").ElementCanvas, "x" | "y" | "collapsed">>, "x" | "y" | "collapsed", "x" | "y">;

export type OrchestratorRef = PlanHydrated<Pick<import("../../../gen/OrchestratorRef").OrchestratorRef, Extract<keyof import("../../../gen/OrchestratorRef").OrchestratorRef, "tab_id" | "session_id" | "title">>, "tab_id" | "session_id" | "title", "tab_id" | "session_id" | "title">;
export type AgentRef = OrchestratorRef;

export type PlannerRef = NullableOptionals<import("../../../gen/PlannerRef").PlannerRef>;

export type PlanMode = import("../../../gen/PlanMode").PlanMode;
export type PlanningState = import("../../../gen/PlanningState").PlanningState;

export type TaskLaneMetadata = PlanHydrated<Pick<import("../../../gen/TaskLaneMetadata").TaskLaneMetadata, Extract<keyof import("../../../gen/TaskLaneMetadata").TaskLaneMetadata, "tab_id" | "workspace_id" | "workspace_name" | "worktree_path" | "branch" | "base" | "destination" | "cleanup_status" | "cleanup_hints">>, never, "tab_id" | "workspace_id" | "workspace_name" | "worktree_path" | "branch" | "base" | "destination" | "cleanup_status">;

export type LaneTurnChangeStats = PlanHydrated<Pick<import("../../../gen/LaneTurnChangeStats").LaneTurnChangeStats, Extract<keyof import("../../../gen/LaneTurnChangeStats").LaneTurnChangeStats, "turn" | "captured_at" | "transcript_turn_key" | "numstat" | "staged_numstat" | "untracked_file_count" | "checkpoint_ref" | "checkpoint_refusal">>, "turn" | "captured_at", "transcript_turn_key" | "numstat" | "staged_numstat" | "untracked_file_count" | "checkpoint_ref" | "checkpoint_refusal">;

export type TransferRecord = Omit<PlanHydrated<Pick<import("../../../gen/TransferRecord").TransferRecord, Extract<keyof import("../../../gen/TransferRecord").TransferRecord, "from" | "to" | "ts" | "reason">>, "from" | "to" | "ts" | "reason", never>, "from" | "to"> & {
  from: OrchestratorRef;
  to: OrchestratorRef;
};

export type PlanLifecycleEvent = Omit<PlanHydrated<Pick<import("../../../gen/PlanLifecycleEvent").PlanLifecycleEvent, Extract<keyof import("../../../gen/PlanLifecycleEvent").PlanLifecycleEvent, "id" | "kind" | "at" | "author">>, "id" | "kind" | "at" | "author", never>, "author"> & {
  author: OrchestratorRef;
};

export type MilestoneKind = PlanHydrated<Pick<import("../../../gen/MilestoneKind").MilestoneKind, Extract<keyof import("../../../gen/MilestoneKind").MilestoneKind, "key" | "label" | "color">>, "key" | "label" | "color", "color">;

export type TaskGroup = PlanHydrated<Pick<import("../../../gen/TaskGroup").TaskGroup, Extract<keyof import("../../../gen/TaskGroup").TaskGroup, "key" | "label" | "color">>, "key" | "label", "color">;

export type TagFacetValue = PlanHydrated<Pick<import("../../../gen/TagFacetValue").TagFacetValue, Extract<keyof import("../../../gen/TagFacetValue").TagFacetValue, "v" | "label" | "color">>, "v", "label" | "color">;

export type TagFacet = Omit<PlanHydrated<Pick<import("../../../gen/TagFacet").TagFacet, Extract<keyof import("../../../gen/TagFacet").TagFacet, "key" | "label" | "color_lane" | "values">>, "key" | "values", "label">, "values"> & {
  values: readonly TagFacetValue[];
};

export type Section = Omit<PlanHydrated<Pick<import("../../../gen/Section").Section, Extract<keyof import("../../../gen/Section").Section, "id" | "title" | "body" | "tags" | "subsections" | "canvas" | "attachments">>, "id" | "title" | "body" | "subsections" | "canvas", never>, "subsections" | "canvas" | "attachments"> & {
  subsections: Section[];
  canvas: ElementCanvas;
  attachments?: PlanAttachment[];
};

export type Milestone = Omit<PlanHydrated<Pick<import("../../../gen/Milestone").Milestone, Extract<keyof import("../../../gen/Milestone").Milestone, "id" | "from" | "kind" | "text" | "ts" | "links" | "loop_id" | "iteration" | "canvas" | "attachments">>, "id" | "from" | "kind" | "text" | "ts" | "links" | "loop_id" | "iteration" | "canvas", "loop_id" | "iteration">, "canvas" | "attachments"> & {
  canvas: ElementCanvas;
  attachments?: PlanAttachment[];
};

export type PlanAttachment = PlanHydrated<Pick<import("../../../gen/AttachmentRef").AttachmentRef, Extract<keyof import("../../../gen/AttachmentRef").AttachmentRef, "id" | "name" | "path" | "media_type" | "size_bytes" | "created_at">>, "id" | "name" | "path" | "media_type" | "size_bytes" | "created_at", never>;

export type TaskPriority = import("../../../gen/TaskPriority").TaskPriority; // p0 = highest
export const TaskPriority = { P0: "p0", P1: "p1", P2: "p2", P3: "p3" } as const satisfies Record<string, TaskPriority>;
export type ReviewStage = import("../../../gen/ReviewStage").ReviewStage;
export type QaState = import("../../../gen/QaState").QaState;

export type TaskCompetitionMetadata = PlanHydrated<Pick<import("../../../gen/TaskCompetitionMetadata").TaskCompetitionMetadata, Extract<keyof import("../../../gen/TaskCompetitionMetadata").TaskCompetitionMetadata, "enabled" | "attempts" | "models" | "judge_hint">>, never, "attempts" | "judge_hint">;

export type TaskExecutionReportSnapshot = PlanHydrated<Pick<import("../../../gen/TaskExecutionReportSnapshot").TaskExecutionReportSnapshot, Extract<keyof import("../../../gen/TaskExecutionReportSnapshot").TaskExecutionReportSnapshot, "id" | "message" | "status">>, never, "message" | "status">;

export type TaskExecutionAttempt = Omit<PlanHydrated<Pick<import("../../../gen/TaskExecutionAttempt").TaskExecutionAttempt, Extract<keyof import("../../../gen/TaskExecutionAttempt").TaskExecutionAttempt, "id" | "tab_id" | "tab_title" | "executor_tab_id" | "executor_title" | "provider" | "agent_type" | "model" | "reasoning_effort" | "actual_reasoning_effort" | "session_model" | "session_id" | "role" | "branch" | "worktree_path" | "report" | "report_status" | "assigned_at" | "reported_at" | "closed_at" | "outcome" | "competition_group_id" | "competition_lane" | "judge_verdict" | "judge_reason" | "judged_at" | "winner" | "lane_turn_stats">>, never, "executor_tab_id" | "executor_title" | "provider" | "agent_type" | "model" | "reasoning_effort" | "actual_reasoning_effort" | "session_model" | "session_id" | "role" | "branch" | "worktree_path" | "report" | "competition_group_id" | "competition_lane" | "judge_verdict" | "judge_reason">, "report" | "lane_turn_stats"> & {
  report?: TaskExecutionReportSnapshot | null;
  lane_turn_stats?: LaneTurnChangeStats[];
  tab_id?: string | null;
  tab_title?: string | null;
  report_status?: string | null;
  outcome?: string | null;
};

export type ExternalRef = PlanHydrated<Pick<import("../../../gen/ExternalRef").ExternalRef, Extract<keyof import("../../../gen/ExternalRef").ExternalRef, "kind" | "repository" | "number" | "canonical_url" | "relationship">>, "kind" | "repository" | "number" | "canonical_url", never>;

export type Task = Omit<PlanHydrated<Pick<import("../../../gen/Task").Task, Extract<keyof import("../../../gen/Task").Task, "criteria_count" | "milestone_count" | "files" | "id" | "name" | "type" | "model" | "model_rationale" | "status" | "review_stage" | "qa_state" | "status_label" | "priority" | "created_at" | "updated_at" | "status_changed_at" | "handle" | "key" | "tags" | "group" | "section_ids" | "body" | "role_guidance" | "executor_role" | "personality" | "acceptance_criteria" | "external_refs" | "test_plan" | "correctness" | "links" | "intent" | "parent_task_id" | "assigned_tab_id" | "lane" | "taken_by_role" | "peer" | "competition" | "execution_attempts" | "pause_reason" | "canvas" | "milestones" | "attachments">>, "id" | "name" | "type" | "model" | "model_rationale" | "status" | "status_label" | "section_ids" | "body" | "intent" | "parent_task_id" | "assigned_tab_id" | "peer" | "canvas" | "milestones", "model" | "model_rationale" | "qa_state" | "status_label" | "handle" | "key" | "group" | "role_guidance" | "executor_role" | "personality" | "intent" | "parent_task_id" | "assigned_tab_id" | "taken_by_role" | "peer" | "competition" | "pause_reason">, "status" | "review_stage" | "qa_state" | "priority" | "external_refs" | "lane" | "competition" | "execution_attempts" | "canvas" | "milestones" | "attachments"> & {
  status: TaskStatus;
  review_stage?: ReviewStage;
  qa_state?: QaState | null;
  priority?: TaskPriority;
  external_refs?: ExternalRef[];
  lane?: TaskLaneMetadata;
  competition?: TaskCompetitionMetadata | null;
  execution_attempts?: TaskExecutionAttempt[];
  canvas: ElementCanvas;
  milestones: Milestone[];
  attachments?: PlanAttachment[];
  criteria_count?: number;
  milestone_count?: number;
  links?: string[];
};

export interface PlanTaskBoardSummary {
  id: string;
  position: number;
  name: string;
  status: TaskStatus;
  created_at: number;
  updated_at: number;
  status_changed_at: number;
  handle?: string | null;
  key?: string | null;
  assigned_tab_id?: string | null;
  lane?: Pick<TaskLaneMetadata, "tab_id" | "branch">;
  taken_by_role?: string | null;
  pause_reason?: string | null;
  model?: string | null;
  peer?: string | null;
  activity_at?: number;
  review_stage?: string | null;
  qa_state?: string | null;
  status_label?: string | null;
  qa_queue_position?: number;
  locator?: PlanTaskBoardLocator | null;
  latest_note?: string | null;
  historical_executor?: PlanTaskHistoricalExecutor | null;
  competition_badge?: PlanTaskCompetitionBadge | null;
  loop_badge?: PlanTaskLoopBadge | null;
  history?: Record<TaskHistoryKind, number | null>;
}

export interface PlanTaskBoardLocator {
  groupKey: string | null;
  groupLabel: string | null;
  groupColor: string | null;
  section: string | null;
}

export interface PlanTaskHistoricalExecutor {
  tab_id: string | null;
  tab_title: string | null;
  role: string | null;
  model: string | null;
  branch: string | null;
  report_status: string | null;
  outcome: string | null;
}

export interface PlanTaskCompetitionAttemptSummary {
  tab_id: string | null;
  tab_title: string | null;
  model: string | null;
  competition_lane: number | null;
}

export interface PlanTaskCompetitionBadge {
  count: number;
  live_count: number;
  judge_hint: string | null;
  models: string[];
  live_attempts: PlanTaskCompetitionAttemptSummary[];
}

export interface PlanTaskLoopBadge {
  count: number;
  budget: number;
  status: LoopSpec["status"];
  stopWhen: string | null;
  warning: boolean;
  tooltip: string;
  loops: LoopSpec[];
}

export interface PlanTaskDetailContext {
  plan_id: string;
  title: string;
  status: PlanStatus;
  team_id?: string | null;
  owner: OrchestratorRef;
  sections: Array<Pick<Section, "id" | "title">>;
  has_open_subtasks?: boolean;
}

export type TaskHistoryKind = import("../../../gen/TaskHistoryKind").TaskHistoryKind;
export interface PlanTaskHistoryPage {
  plan_id: string; task_id: string; revision: string;
  items: Array<Milestone | TaskExecutionAttempt>;
  next_cursor: number | null;
}

export interface PlanTaskDetail {
  revision?: string;
  plan_id: string;
  task_id: string;
  task: Task;
  context: PlanTaskDetailContext;
  loop_badge?: PlanTaskLoopBadge | null;
  history?: Record<TaskHistoryKind, number | null>;
}

export type PlanTaskBoardColumnKey = "pending" | "in_progress" | "in_review" | "blocked" | "done";
export type PlanTaskBoardStatus = PlanTaskBoardColumnKey | "cancelled";

export interface PlanTaskBoardCursor {
  created_at: number;
  position: number;
  task_id: string;
}

export interface PlanTaskBoardColumnPage {
  status: PlanTaskBoardStatus;
  items: PlanTaskBoardSummary[];
  next_cursor: PlanTaskBoardCursor | null;
}

export interface PlanTaskBoardCounts {
  pending: number;
  in_progress: number;
  in_review: number;
  blocked: number;
  done: number;
  cancelled: number;
  total: number;
}

export interface PlanTaskBoardPage {
  revision?: string;
  plan_id: string;
  counts?: PlanTaskBoardCounts;
  columns?: Partial<Record<PlanTaskBoardStatus, PlanTaskBoardColumnPage>>;
  column?: PlanTaskBoardColumnPage;
}

export interface LoopStage {
  key: string;
  role: string;
  model: string | null;
  why: string | null;
  prompt: string | null;
}

export type LoopSpec = Omit<PlanHydrated<Pick<import("../../../gen/LoopSpec").LoopSpec, Extract<keyof import("../../../gen/LoopSpec").LoopSpec, "id" | "target" | "stages" | "budget" | "count" | "stop_when" | "status" | "stagnation" | "deviation" | "last_iteration_at">>, "id" | "target" | "budget" | "count" | "stop_when" | "status", "stop_when" | "deviation" | "last_iteration_at">, "status"> & {
  status: LoopStatus;
  stages?: LoopStage[];
};

export type CanvasElement = Omit<PlanHydrated<Pick<import("../../../gen/CanvasElement").CanvasElement, Extract<keyof import("../../../gen/CanvasElement").CanvasElement, "id" | "kind" | "canvas" | "content" | "links">>, "id" | "kind" | "canvas" | "content" | "links", never>, "canvas"> & {
  canvas: ElementCanvas;
};

export type Viewport = PlanHydrated<Pick<import("../../../gen/Viewport").Viewport, Extract<keyof import("../../../gen/Viewport").Viewport, "pan_x" | "pan_y" | "scale">>, "pan_x" | "pan_y" | "scale", never>;

export type PlanCanvas = Omit<PlanHydrated<Pick<import("../../../gen/PlanCanvas").PlanCanvas, Extract<keyof import("../../../gen/PlanCanvas").PlanCanvas, "viewport">>, "viewport", "viewport">, "viewport"> & {
  viewport: Viewport | null;
};

export type Goal = PlanHydrated<Pick<import("../../../gen/Goal").Goal, Extract<keyof import("../../../gen/Goal").Goal, "vector" | "vision" | "constraints" | "postulates">>, "vector", never>;

export type HeartbeatLearning = PlanHydrated<Pick<import("../../../gen/HeartbeatLearning").HeartbeatLearning, Extract<keyof import("../../../gen/HeartbeatLearning").HeartbeatLearning, "id" | "text" | "ts" | "tag">>, "id" | "text" | "ts", "tag">;

export type OrchestratorHeartbeat = Omit<PlanHydrated<Pick<import("../../../gen/OrchestratorHeartbeat").OrchestratorHeartbeat, Extract<keyof import("../../../gen/OrchestratorHeartbeat").OrchestratorHeartbeat, "current_focus" | "active_lanes" | "rules" | "learnings">>, never, "current_focus">, "learnings"> & {
  learnings?: HeartbeatLearning[];
};

export type SpecLink = PlanHydrated<Pick<import("../../../gen/SpecLink").SpecLink, Extract<keyof import("../../../gen/SpecLink").SpecLink, "spec_id" | "spec_section_id" | "plan_section_id">>, "spec_id" | "spec_section_id" | "plan_section_id", never>;

export type EffortLinkRel = import("../../../gen/EffortLinkRel").EffortLinkRel;

export type EffortLink = Omit<PlanHydrated<Pick<import("../../../gen/EffortLink").EffortLink, Extract<keyof import("../../../gen/EffortLink").EffortLink, "to" | "rel">>, "to" | "rel", never>, "rel"> & {
  rel: EffortLinkRel;
};

export type Plan = Omit<PlanHydrated<Pick<import("../../../gen/Plan").Plan, Extract<keyof import("../../../gen/Plan").Plan, "task_counts" | "control_loop" | "pause_operations" | "id" | "title" | "sections" | "status" | "mode" | "planning_state" | "owner" | "planner" | "team_id" | "project_path" | "created_at" | "updated_at" | "milestone_kinds" | "task_groups" | "tag_facets" | "loops" | "elements" | "tasks" | "canvas" | "owner_history" | "lifecycle_events" | "goal" | "heartbeat" | "primary_spec_id" | "spec_links" | "links" | "lineage" | "strides" | "gates">>, "id" | "title" | "sections" | "status" | "owner" | "project_path" | "created_at" | "updated_at" | "milestone_kinds" | "loops" | "elements" | "tasks" | "canvas" | "owner_history", "planner" | "team_id" | "goal" | "primary_spec_id">, "control_loop" | "pause_operations" | "sections" | "status" | "mode" | "planning_state" | "owner" | "planner" | "milestone_kinds" | "task_groups" | "tag_facets" | "loops" | "elements" | "tasks" | "canvas" | "owner_history" | "lifecycle_events" | "goal" | "heartbeat" | "spec_links" | "lineage" | "strides" | "gates"> & {
  control_loop?: import("./planControlLoop").PlanControlLoop;
  pause_operations?: Array<Pick<import("../../../gen/PlanPauseOperation").PlanPauseOperation, "paused_at" | "reason"> & Partial<Omit<import("../../../gen/PlanPauseOperation").PlanPauseOperation, "paused_at" | "reason">>>;
  sections: Section[];
  status: PlanStatus;
  mode?: PlanMode;
  planning_state?: PlanningState;
  owner: OrchestratorRef;
  planner?: PlannerRef | null;
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
  spec_links?: SpecLink[];
  lineage?: EffortLink[];
  strides?: Milestone[];
  gates?: DecisionGate[];
  task_counts?: Partial<Record<TaskStatus, number>> & { total: number };
};

export type GateStatus = import("../../../gen/GateStatus").GateStatus;

export type GateAnswer = PlanHydrated<Pick<import("../../../gen/GateAnswer").GateAnswer, Extract<keyof import("../../../gen/GateAnswer").GateAnswer, "text" | "by_tab_id" | "at">>, "text" | "at", "by_tab_id">;

/** Structured decision request that pauses blocking tasks until answered. */
export type DecisionGate = Omit<PlanHydrated<Pick<import("../../../gen/DecisionGate").DecisionGate, Extract<keyof import("../../../gen/DecisionGate").DecisionGate, "id" | "question" | "context" | "blocking_task_ids" | "routed_to_tab_id" | "asked_by_tab_id" | "asked_at" | "status" | "answer" | "escalated_at" | "escalation_note">>, "id" | "question" | "asked_at" | "status", "routed_to_tab_id" | "asked_by_tab_id" | "answer" | "escalated_at" | "escalation_note">, "status" | "answer"> & {
  status: GateStatus;
  answer?: GateAnswer | null;
};
