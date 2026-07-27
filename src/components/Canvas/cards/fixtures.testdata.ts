import type { DriftItem, Milestone, Plan, Section, Task } from "../types";

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
