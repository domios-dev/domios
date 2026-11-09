import type { Plan, Section, Task } from "./types";

export type PlanProjectionView = import("../../../gen/PlanViewProjection").PlanViewProjection;
export type PlanTaskView = Pick<Task,
  "id" | "name" | "type" | "status" | "model" | "status_label" |
  "parent_task_id" | "assigned_tab_id" | "peer" | "section_ids" | "canvas"
> & Partial<Omit<Task,
  "id" | "name" | "type" | "status" | "model" | "status_label" |
  "parent_task_id" | "assigned_tab_id" | "peer" | "section_ids" | "canvas"
>>;

export type PlanSectionView = Pick<Section, "id" | "title"> & {
  subsections: PlanSectionView[];
} & Partial<Omit<Section, "id" | "title" | "subsections">>;

export type PlanView = Pick<Plan,
  "id" | "title" | "status" | "owner" | "project_path" | "created_at" | "updated_at" | "canvas"
> & Partial<Omit<Plan,
  "id" | "title" | "status" | "owner" | "project_path" | "created_at" | "updated_at" | "canvas" |
  "tasks" | "sections"
>> & { tasks: PlanTaskView[]; sections: PlanSectionView[] };

export type PlanTaskMetadata = Pick<PlanTaskView,
  "id" | "name" | "type" | "key" | "handle" | "status" | "status_label" | "review_stage" | "qa_state" |
  "assigned_tab_id" | "lane" | "parent_task_id" | "section_ids" | "group" | "model" | "peer" |
  "priority" | "created_at" | "updated_at" | "status_changed_at" | "tags" | "canvas" |
  "execution_attempts" | "criteria_count" | "milestone_count"
> & { milestones?: Task["milestones"]; acceptance_criteria?: string[] };

export type PlanProjection = Pick<PlanView,
  "id" | "title" | "status" | "owner" | "planner" | "project_path" | "created_at" | "updated_at" |
  "mode" | "planning_state" | "team_id" | "canvas" | "goal" | "primary_spec_id" | "spec_links" |
  "task_groups" | "tag_facets" | "milestone_kinds" | "heartbeat" | "task_counts"
> & { revision: string; canonical_project_path?: string; spec_refs?: string[];
  tasks: PlanTaskMetadata[]; sections: PlanSectionView[] };

export type CommittedPlanChange = import("../../../gen/CommittedPlanChange").CommittedPlanChange;

export function planRevision(value: unknown): string {
  const revision = value && typeof value === "object" && "revision" in value ? value.revision : null;
  if (typeof revision !== "string" || !/^\d{1,19}$/.test(revision)) {
    throw new Error("Plan projection has no valid committed revision.");
  }
  return revision;
}

export function comparePlanRevisions(left: string, right: string): number {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

export interface PlanListCursor { position: number; task_id: string }
export type PlanListProjection = PlanProjection & { task_window: {
  next_cursor: PlanListCursor | null; total: number; cancelled: number;
} };
