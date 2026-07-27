import type { Workspace } from "../../lib/types";
import type { Plan, Task, TaskLaneMetadata } from "./types";

export interface PlanWorkspaceScope {
  ownerWorkspaceId: string | null;
  ownerWorkspaceLabel: string | null;
  participantWorkspaceLabels: string[];
  isCrossWorkspace: boolean;
}

interface WorkspaceCandidate {
  id: string | null;
  label: string;
}

const EMPTY_LABELS = new Set(["", "unknown", "unowned", "none", "null"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function workspaceByTab(workspaces: readonly Workspace[]): Map<string, WorkspaceCandidate> {
  const byTab = new Map<string, WorkspaceCandidate>();
  for (const workspace of workspaces) {
    const id = clean(workspace.id);
    const label = clean(workspace.name) ?? id;
    if (!label) continue;
    for (const tabId of workspace.tabs) {
      byTab.set(tabId, { id, label });
    }
  }
  return byTab;
}

function candidateKey(candidate: WorkspaceCandidate): string {
  return candidate.id ? `id:${candidate.id}` : `label:${candidate.label.toLowerCase()}`;
}

function laneWorkspace(lane?: TaskLaneMetadata): WorkspaceCandidate | null {
  const id = clean(lane?.workspace_id);
  const label = clean(lane?.workspace_name) ?? id;
  return label ? { id, label } : null;
}

function taskWorkspace(task: Task, byTab: Map<string, WorkspaceCandidate>): WorkspaceCandidate | null {
  const lane = laneWorkspace(task.lane);
  if (lane) return lane;
  const tabId = clean(task.assigned_tab_id) ?? clean(task.lane?.tab_id);
  return tabId ? byTab.get(tabId) ?? null : null;
}

function ownerWorkspaceFromLane(plan: Plan): WorkspaceCandidate | null {
  const ownerTabId = clean(plan.owner.tab_id);
  if (!ownerTabId) return null;
  for (const task of plan.tasks) {
    if (clean(task.lane?.tab_id) !== ownerTabId) continue;
    const lane = laneWorkspace(task.lane);
    if (lane) return lane;
  }
  return null;
}

export function derivePlanWorkspaceScope(plan: Plan, workspaces: readonly Workspace[] = []): PlanWorkspaceScope {
  const byTab = workspaceByTab(workspaces);
  const ownerTabId = clean(plan.owner.tab_id);
  const ownerWorkspace = ownerTabId ? byTab.get(ownerTabId) ?? ownerWorkspaceFromLane(plan) : null;
  const participants = new Map<string, WorkspaceCandidate>();
  if (ownerWorkspace) participants.set(candidateKey(ownerWorkspace), ownerWorkspace);
  for (const task of plan.tasks) {
    const workspace = taskWorkspace(task, byTab);
    if (workspace) participants.set(candidateKey(workspace), workspace);
  }
  return {
    ownerWorkspaceId: ownerWorkspace?.id ?? null,
    ownerWorkspaceLabel: ownerWorkspace?.label ?? null,
    participantWorkspaceLabels: [...participants.values()].map((w) => w.label),
    isCrossWorkspace: participants.size > 1,
  };
}
