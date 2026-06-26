import type { Pane, Workspace } from "../../lib/types";
import { childPaneIdsInWorkspace, paneIdsInWorkspace } from "../Sidebar/workspaceTabs";
import type { PlanIndexEntry } from "./plansApi";
import { assignedTaskIdsForPane } from "./planOpenFocus";
import { findPlanIndexForPane } from "./planPaneMatch";
import { canvasFocusStore, type CanvasWorkspaceTarget } from "./canvasFocusStore";
import type { Plan } from "./types";

export function workspacePaneIds(workspace: Workspace, panes: Record<string, Pane>): Set<string> {
  const ids = paneIdsInWorkspace(workspace.tabs, panes);
  for (const pid of childPaneIdsInWorkspace(workspace.tabs, panes)) ids.add(pid);
  return ids;
}

export function deriveWorkspacePlanEntries(
  index: PlanIndexEntry[],
  workspace: Workspace,
  panes: Record<string, Pane>,
): PlanIndexEntry[] {
  const paneIds = workspacePaneIds(workspace, panes);
  const active = index.filter((e) => e.status === "active");
  const seen = new Set<string>();
  const out: PlanIndexEntry[] = [];
  for (const entry of active) {
    if (!planBelongsToWorkspacePath(entry, workspace)) continue;
    seen.add(entry.plan_id);
    out.push(entry);
  }
  for (const paneId of paneIds) {
    for (const entry of findPlanIndexForPane(active, paneId, panes)) {
      if (seen.has(entry.plan_id)) continue;
      seen.add(entry.plan_id);
      out.push(entry);
    }
  }
  return out.sort((a, b) => b.updated_at - a.updated_at);
}

function planBelongsToWorkspacePath(entry: PlanIndexEntry, workspace: Workspace): boolean {
  if (!workspace.cwd) return false;
  const root = normalizePath(workspace.cwd);
  const file = normalizePath(entry.file_path);
  return file === `${root}/.codeterm/plans/${entry.plan_id}.json`
    || file.startsWith(`${root}/.codeterm/plans/`);
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

export function workspacePlanEntryLabel(entries: PlanIndexEntry[]): string {
  if (entries.length === 0) return "Plan";
  if (entries.length === 1) return entries[0].title.trim() || "Plan";
  return `Plans (${entries.length})`;
}

export function resolvePanePlanFocusTarget(
  entries: PlanIndexEntry[],
  paneId: string,
): CanvasWorkspaceTarget | null {
  if (entries.length === 0) return null;
  return {
    focusPaneId: paneId,
    planId: entries.length === 1 ? entries[0].plan_id : undefined,
  };
}

export function openWorkspacePlanCanvas(target?: CanvasWorkspaceTarget): void {
  if (target?.planId) canvasFocusStore.clearPulse(target.planId);
  canvasFocusStore.resetWorkspaceDismissed();
  canvasFocusStore.openWorkspace(target ?? undefined);
}

export function openWorkspacePlanForEntries(entries: readonly PlanIndexEntry[]): void {
  openWorkspacePlanCanvas({
    planId: entries.length === 1 ? entries[0].plan_id : undefined,
  });
}

export function resolveWorkspaceFocusNodeIds(
  mode: "workspace" | "focus",
  target: CanvasWorkspaceTarget | null | undefined,
  plans: readonly Plan[],
): string[] {
  if (mode !== "workspace" || !target) return [];
  if (target.nodeIds?.length) return target.nodeIds;
  if (target.focusPaneId) {
    const taskIds = assignedTaskIdsForPane(plans, target.focusPaneId);
    if (taskIds.length) return taskIds;
  }
  if (target.planId) return [target.planId];
  return [];
}

export function canvasFitContextKey(
  mode: string,
  focusPaneId: string | null | undefined,
  workspaceTarget: CanvasWorkspaceTarget | null | undefined,
): string {
  const ws = workspaceTarget;
  return `${mode}:${focusPaneId ?? ""}:${ws?.focusPaneId ?? ""}:${ws?.planId ?? ""}:${ws?.nodeIds?.join(",") ?? ""}`;
}

export function openPanePlanFocus(
  entries: PlanIndexEntry[],
  paneId: string,
  nodeIds?: string[],
): void {
  const base = resolvePanePlanFocusTarget(entries, paneId);
  if (!base) return;
  openWorkspacePlanCanvas({
    ...base,
    nodeIds: nodeIds?.length ? nodeIds : undefined,
  });
}
