import type { Plan, Task } from "./types";

export type PlanProjectionView = import("../../../gen/PlanViewProjection").PlanViewProjection;
export type PlanTaskMetadata = import("../../../gen/PlanTaskMetadata").PlanTaskMetadata;
export type PlanRead = import("../../../gen/PlanView").PlanView;
export type PlanSummaryRead = import("../../../gen/PlanSummaryView").PlanSummaryView;
export type PlanDetailedRead = import("../../../gen/PlanDetailedView").PlanDetailedView;
export type PlanChatRead = import("../../../gen/ChatPlanRead").ChatPlanRead;
export type PlanProjection = import("../../../gen/PlanProjection").PlanProjection;
export type PlanReadForView<V extends PlanProjectionView> = V extends "chat" ? PlanChatRead :
  V extends "summary" | "task_metadata" ? PlanSummaryRead : PlanDetailedRead;

type TaskIdentity = "id" | "name" | "type" | "status" | "model" | "status_label" |
  "parent_task_id" | "assigned_tab_id" | "peer" | "section_ids" | "canvas";
export type PlanTaskView = Pick<PlanTaskMetadata, TaskIdentity> & Omit<Partial<Task>, "lane"> &
  { lane?: Task["lane"] | PlanTaskMetadata["lane"] };
export type PlanSectionView = import("../../../gen/PlanSectionView").PlanSectionView;
type PlanIdentity = "id" | "title" | "status" | "owner" | "project_path" | "created_at" | "updated_at" | "canvas";
export type PlanView = Pick<PlanRead, PlanIdentity> &
  Partial<Omit<Plan, PlanIdentity | "tasks" | "sections">> &
  { tasks: PlanTaskView[]; sections: PlanSectionView[] };

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

export type PlanListCursor = import("../../../gen/PlanListCursor").PlanListCursor;
export type PlanListProjection = PlanDetailedRead & Required<Pick<PlanDetailedRead, "task_window">>;
export type PlanMetadataProjection = PlanSummaryRead & Required<Pick<PlanSummaryRead, "task_window">>;
