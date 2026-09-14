import type { Workspace } from "../../lib/types";
import type { PlanIndexEntry, ResolvedPlanRef } from "../Canvas/plansApi";
import { taskReferenceLabel } from "../Canvas/taskDisplayKey";
import { findPlanOwnerWorkspace } from "../Canvas/workspacePlanEntry";
import { planRefViewState } from "./planRefViewState";

function refTitle(resolved: ResolvedPlanRef): string {
  if (resolved.target_kind !== "task" || !resolved.target_id || !resolved.task_title) return resolved.title;
  return taskReferenceLabel({ id: resolved.target_id, key: resolved.task_key, name: resolved.task_title });
}

export function chatPlanRefView(
  resolved: ResolvedPlanRef,
  planIndex: readonly PlanIndexEntry[],
  workspaces: readonly Workspace[],
  resolverUnreachable: boolean,
) {
  const ownerWorkspace = findPlanOwnerWorkspace(
    resolved.plan_id,
    planIndex,
    workspaces,
    resolved.canonical_project_path,
  );
  return {
    title: refTitle(resolved),
    planTitle: resolved.plan_title,
    taskTitle: resolved.task_title,
    sectionTitle: resolved.section_title,
    state: planRefViewState(resolved.state, ownerWorkspace != null, resolverUnreachable),
    rawLabel: resolved.target_id ?? resolved.plan_id,
    fullRef: resolved.target_id
      ? `${resolved.plan_id}/${resolved.target_id}`
      : resolved.plan_id,
  };
}
