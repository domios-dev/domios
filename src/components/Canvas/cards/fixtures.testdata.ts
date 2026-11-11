import type { PlanTaskMetadata, PlanSummaryRead, PlanDetailedRead } from "../planProjection";
import type { PlanBoardState } from "../missionControl/missionControlState";
import type { DriftItem, Milestone, Plan, PlanTaskBoardSummary, Section, Task } from "../types";

const emptyCanvas = { x: null, y: null, collapsed: false };

export const FIXTURE_SECTION: Section = {
  id: "sec-auth",
  title: "Authentication",
  body: "Users **sign in** via OAuth.",
  subsections: [
    {
      id: "sec-auth-oauth",
      title: "OAuth flow",
      body: "Redirect to provider.",
      subsections: [],
      canvas: emptyCanvas,
    },
  ],
  canvas: emptyCanvas,
};

export const FIXTURE_TAG_FACETS = [
  {
    key: "area",
    label: "Area",
    color_lane: true,
    values: [{ v: "nav", label: "Navigation", color: "rgba(99, 102, 241, 0.95)" }],
  },
] as const;

export const FIXTURE_TASK: Task = {
  id: "task-dante",
  name: "Dante",
  type: "implement",
  model: "claude:opus",
  model_rationale: "Complex auth logic needs a strong model.",
  status: "in_progress",
  status_label: "90% done",
  section_ids: ["sec-auth"],
  body: "Implement the **login** endpoint.",
  intent: null,
  parent_task_id: null,
  assigned_tab_id: null,
  peer: null,
  tags: ["area:nav", "pan-zoom"],
  canvas: emptyCanvas,
  milestones: [],
};

export const FIXTURE_MILESTONE: Milestone = {
  id: "ms-1",
  from: { tab_id: "p1", session_id: null, title: "Worker" },
  kind: "status_update",
  text: "Initial scaffold **landed**.",
  ts: 1_700_000_000_000,
  links: [],
  loop_id: null,
  iteration: 2,
  canvas: emptyCanvas,
};

export const FIXTURE_DRIFT: DriftItem = {
  task_id: "task-dante",
  plan_id: "plan-alpha",
  kind: "in_progress_tab_gone",
  detail: "Task is in progress but the worker Tab is gone.",
  suggested_fix: "Move it back to pending or spawn a replacement worker.",
  not_mine: false,
};

export const FIXTURE_NOT_MINE_DRIFT: DriftItem = {
  ...FIXTURE_DRIFT,
  kind: "bound_to_closed_tab",
  detail: "Task is bound to a closed Tab owned by another orchestrator.",
  suggested_fix: "Ask that orchestrator to reconcile the task.",
  not_mine: true,
};

export const FIXTURE_PLAN: Plan = {
  id: "plan-alpha",
  title: "Auth rollout",
  sections: [FIXTURE_SECTION],
  status: "active",
  owner: { tab_id: "orch-1", session_id: "sess-1", title: "Orchestrator" },
  project_path: "/tmp/project",
  created_at: 1_700_000_000_000,
  updated_at: 1_700_000_100_000,
  milestone_kinds: [{ key: "status_update", label: "Status", color: null }],
  tag_facets: [...FIXTURE_TAG_FACETS],
  loops: [],
  elements: [],
  tasks: [FIXTURE_TASK],
  canvas: { viewport: null },
  owner_history: [],
};


export const FIXTURE_TASK_METADATA: PlanTaskMetadata = {
  id: FIXTURE_TASK.id, name: FIXTURE_TASK.name, type: FIXTURE_TASK.type,
  key: null, handle: null, status: FIXTURE_TASK.status, status_label: FIXTURE_TASK.status_label,
  review_stage: "review", qa_state: null, assigned_tab_id: null, lane: null,
  parent_task_id: null, depends_on: [], section_ids: FIXTURE_TASK.section_ids, group: null,
  model: FIXTURE_TASK.model, peer: null, priority: "p2", created_at: 0, updated_at: 0,
  status_changed_at: 0, tags: [], canvas: FIXTURE_TASK.canvas, execution_attempts: [],
  criteria_count: 0, milestone_count: 0,
};

export const FIXTURE_PLAN_READ: PlanSummaryRead = {
  id: FIXTURE_PLAN.id, title: FIXTURE_PLAN.title, status: FIXTURE_PLAN.status,
  owner: FIXTURE_PLAN.owner, project_path: FIXTURE_PLAN.project_path,
  canonical_project_path: FIXTURE_PLAN.project_path, created_at: FIXTURE_PLAN.created_at,
  updated_at: FIXTURE_PLAN.updated_at, canvas: FIXTURE_PLAN.canvas, planner: null,
  mode: "planning", planning_state: "drafting", team_id: null, revision: "1",
  tasks: [], sections: [],
  task_counts: { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, total: 0 },
};

export const FIXTURE_PLAN_DETAILED_READ: PlanDetailedRead = {
  ...FIXTURE_PLAN_READ, goal: null, spec_links: [], primary_spec_id: null, spec_refs: [],
  task_groups: [], tag_facets: [], milestone_kinds: [],
};

export const FIXTURE_BOARD_SUMMARY: PlanTaskBoardSummary = {
  id: FIXTURE_TASK.id, name: FIXTURE_TASK.name, status: FIXTURE_TASK.status,
  position: 0, created_at: 0, updated_at: 0, status_changed_at: 0, activity_at: 0,
  handle: null, key: null, assigned_tab_id: null, lane: { tab_id: null, branch: null },
  taken_by_role: null, pause_reason: null, model: null, peer: null, review_stage: null,
  qa_state: null, status_label: null, qa_queue_position: 0, locator: null, latest_note: null,
  historical_executor: null, competition_badge: null, loop_badge: null,
};

export const FIXTURE_BOARD_READ_FIELDS: Pick<PlanBoardState,
  "revision" | "control_loop" | "next_event_cursor" | "event_window_loaded"
> = {
  revision: "1", next_event_cursor: null, event_window_loaded: true,
  control_loop: {
    mode: "curator", heartbeat_interval_secs: null, next_at: null, last_round: null,
    fallback_reason: null, manual_run_requested: false, running: null, suspended_reason: null,
    counts_7d: { rounds: 0, quiet: 0, nudged: 0, learnings: 0, saves: 0, fallbacks: 0 },
  },
};
