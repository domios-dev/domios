import type { Plan } from "./types";

export type PlanProjectionView = "summary" | "board" | "document" | "list" | "chat" | "task_metadata";
export type PlanProjection = Plan & { revision: string };

export interface CommittedPlanChange {
  plan_id: string;
  revision: string;
  previous_revision: string;
  task_ids: string[];
  reset: boolean;
  deleted: boolean;
}

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
