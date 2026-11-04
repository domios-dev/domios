import type { Workspace } from "../../lib/types";
import type { PlanIndexEntry, ResolvedPlanRef } from "../Canvas/plansApi";
import { taskDisplayKey, taskReferenceLabel } from "../Canvas/taskDisplayKey";
import { findPlanOwnerWorkspace } from "../Canvas/workspacePlanEntry";
import { planRefViewState } from "./planRefViewState";

function refTitle(resolved: ResolvedPlanRef): string {
  if (resolved.target_kind === "plan" || !resolved.target_id) return resolved.title;
  if (resolved.target_kind === "task" && resolved.task_title) return taskReferenceLabel({ id: resolved.target_id, key: resolved.task_key, name: resolved.task_title });
  if (resolved.target_kind === "section" && resolved.section_title) return resolved.title;
  // A task or section the plan no longer has keeps its own id; the plan's title would name the wrong thing.
  return resolved.task_key?.trim() || resolved.target_id;
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
    taskKey: resolved.target_kind === "task" && resolved.target_id
      ? taskDisplayKey({ id: resolved.target_id, key: resolved.task_key })
      : undefined,
    sectionTitle: resolved.section_title,
    state: planRefViewState(resolved.state, ownerWorkspace != null, resolverUnreachable),
    rawLabel: resolved.target_id ?? resolved.plan_id,
    fullRef: resolved.target_id
      ? `${resolved.plan_id}/${resolved.target_id}`
      : resolved.plan_id,
  };
}
