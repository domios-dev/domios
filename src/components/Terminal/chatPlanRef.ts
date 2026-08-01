import type { Workspace } from "../../lib/types";
import type { PlanIndexEntry, ResolvedPlanRef } from "../Canvas/plansApi";
import { findPlanOwnerWorkspace } from "../Canvas/workspacePlanEntry";
import { planRefViewState } from "./planRefViewState";

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
    title: resolved.title,
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
