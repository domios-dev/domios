import { useCallback } from "react";
import type { Team, Workspace } from "../../lib/types";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import {
  planNavContextEqual,
  selectPlanNavContext,
  selectTeams,
  teamsEqual,
} from "../../store/multiplexer/selectors";
import { planSidePanelStore } from "./planSidePanelStore";
import type { PlanIndexEntry } from "./plansApi";
import { usePlanIndexOptional } from "./PlansProvider";
import { findPlanOwnerWorkspace, openWorkspacePlanCanvas } from "./workspacePlanEntry";

/** "side" previews over the current surface without navigating; "full" lands in the team view. */
export type PlanTargetPresentation = "side" | "full";

export interface PlanTarget {
  planId: string;
  taskId?: string;
  /** The plan's team when the caller already holds the plan; the index supplies it otherwise. */
  teamId?: string | null;
  /** Daemon-resolved ownership for a plan the index has not loaded yet. */
  canonicalProjectPath?: string;
  /** Defaults to "side". */
  presentation?: PlanTargetPresentation;
}

export interface PlanTargetContext {
  index: readonly PlanIndexEntry[];
  workspaces: readonly Workspace[];
  teams: readonly Team[];
  activeWorkspaceId: string | null;
  setActiveWorkspace: (id: string) => void;
}

export interface PlanTargetOwner {
  workspaceId: string;
  teamId?: string;
}

export function resolvePlanTargetOwner(target: PlanTarget, ctx: PlanTargetContext): PlanTargetOwner | null {
  const teamId = target.teamId ?? ctx.index.find((entry) => entry.plan_id === target.planId)?.team_id ?? undefined;
  const team = teamId ? ctx.teams.find((candidate) => candidate.id === teamId) : undefined;
  if (team && ctx.workspaces.some((workspace) => workspace.id === team.workspace_id)) {
    return { workspaceId: team.workspace_id, teamId: team.id };
  }
  const owner = findPlanOwnerWorkspace(target.planId, ctx.index, ctx.workspaces, target.canonicalProjectPath);
  return owner ? { workspaceId: owner.workspace.id, teamId } : null;
}

/** The one opening path for plans and tasks: the side panel by default, the owner's team view on "full". */
export function openPlanTarget(target: PlanTarget, ctx: PlanTargetContext): boolean {
  const owner = resolvePlanTargetOwner(target, ctx);
  if (!owner) return false;
  if ((target.presentation ?? "side") === "side") {
    planSidePanelStore.open({
      planId: target.planId,
      taskId: target.taskId,
      teamId: owner.teamId ?? null,
      canonicalProjectPath: target.canonicalProjectPath,
    });
    return true;
  }
  planSidePanelStore.close();
  if (owner.workspaceId !== ctx.activeWorkspaceId) ctx.setActiveWorkspace(owner.workspaceId);
  openWorkspacePlanCanvas({
    teamId: owner.teamId,
    planId: target.planId,
    jump: target.taskId ? { view: "task", taskId: target.taskId } : { view: "plan" },
  });
  return true;
}

export function usePlanTargetNavigation(): (target: PlanTarget) => boolean {
  const { setActiveWorkspace } = useMultiplexer();
  const nav = useMuxSelector(selectPlanNavContext, planNavContextEqual);
  const teams = useMuxSelector(selectTeams, teamsEqual);
  const { index } = usePlanIndexOptional();
  return useCallback(
    (target: PlanTarget) => openPlanTarget(target, {
      index,
      workspaces: nav.workspaces,
      teams,
      activeWorkspaceId: nav.activeWorkspaceId,
      setActiveWorkspace,
    }),
    [index, nav.workspaces, nav.activeWorkspaceId, teams, setActiveWorkspace],
  );
}
