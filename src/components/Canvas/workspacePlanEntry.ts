import type { Tab, Workspace } from "../../lib/types";
import { childTabIdsInWorkspace, tabIdsInWorkspace } from "../Sidebar/workspaceTabs";
import type { PlanIndexEntry } from "./plansApi";
import { assignedTaskIdsForTab } from "./planOpenFocus";
import { canvasFocusStore, type CanvasWorkspaceTarget } from "./canvasFocusStore";
import type { Plan } from "./types";

export function workspaceTabIds(workspace: Workspace, tabs: Record<string, Tab>): Set<string> {
  const ids = tabIdsInWorkspace(workspace.tabs, tabs);
  for (const pid of childTabIdsInWorkspace(workspace.tabs, tabs)) ids.add(pid);
  return ids;
}

// Cumulative workspace view: the root's own plans plus nested project roots.
// Deduped by canonical plan id and ordered deterministically across reloads.
export function deriveWorkspacePlanEntries(
  index: PlanIndexEntry[],
  workspace: Workspace,
  _tabs: Record<string, Tab>,
): PlanIndexEntry[] {
  const canonical = new Map<string, PlanIndexEntry>();
  for (const entry of index) {
    if (entry.status !== "active") continue;
    if (!planBelongsToWorkspacePath(entry, workspace) || !hasWorkspacePlanContent(entry)) continue;
    const prev = canonical.get(entry.plan_id);
    if (!prev || isFresherEntry(entry, prev)) canonical.set(entry.plan_id, entry);
  }
  return [...canonical.values()].sort(compareWorkspacePlanEntries);
}

function isFresherEntry(a: PlanIndexEntry, b: PlanIndexEntry): boolean {
  if (a.updated_at !== b.updated_at) return a.updated_at > b.updated_at;
  return normalizePath(a.file_path) < normalizePath(b.file_path);
}

function compareWorkspacePlanEntries(a: PlanIndexEntry, b: PlanIndexEntry): number {
  if (a.updated_at !== b.updated_at) return b.updated_at - a.updated_at;
  return a.plan_id < b.plan_id ? -1 : a.plan_id > b.plan_id ? 1 : 0;
}

function planBelongsToWorkspacePath(entry: PlanIndexEntry, workspace: Workspace): boolean {
  if (!workspace.cwd) return false;
  const canonicalProjectPath = entry.canonical_project_path?.trim();
  if (canonicalProjectPath) {
    return projectPathBelongsToWorkspace(canonicalProjectPath, workspace.cwd);
  }
  const legacyProjectPath = legacyPlanProjectPath(entry);
  return legacyProjectPath != null
    && projectPathBelongsToWorkspace(legacyProjectPath, workspace.cwd);
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "");
}

function comparablePath(path: string): string {
  const normalized = normalizePath(path);
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}

function projectPathBelongsToWorkspace(projectPath: string, workspacePath: string): boolean {
  const project = comparablePath(projectPath);
  const root = comparablePath(workspacePath);
  return project === root || project.startsWith(`${root}/`);
}

function legacyPlanProjectPath(entry: PlanIndexEntry): string | null {
  const file = normalizePath(entry.file_path);
  const marker = "/.codeterm/plans/";
  const markerIndex = file.lastIndexOf(marker);
  if (markerIndex < 0) return null;
  const parts = file.slice(markerIndex + marker.length).split("/");
  if (parts.length !== 3) return null;
  const [bucket, planId, fileName] = parts;
  if ((bucket !== "active" && bucket !== "archive") || planId !== entry.plan_id) return null;
  if (fileName !== "plan.json" && fileName !== "plan.yaml") return null;
  return file.slice(0, markerIndex);
}

function hasWorkspacePlanContent(entry: PlanIndexEntry): boolean {
  return (entry.task_count ?? 0) > 0
    || (entry.section_count ?? 0) > 0
    || (entry.element_count ?? 0) > 0
    || (entry.loop_count ?? 0) > 0
    || entry.has_goal === true
    || entry.assigned_tab_ids.length > 0;
}

export function workspacePlanEntryKind(_entries: PlanIndexEntry[]): string {
  return "Plan";
}

export function workspacePlanEntryLabel(_entries: PlanIndexEntry[]): string {
  return "Plan";
}

export function workspacePlanEntryTitle(entries: PlanIndexEntry[]): string {
  if (entries.length === 0) return "Open workspace plan";
  if (entries.length === 1) {
    const title = entries[0].title.trim();
    return title ? `Open plan: ${title}` : "Open workspace plan";
  }
  const lead = entries[0]?.title.trim() || "Untitled plan";
  const rest = entries.length - 1;
  return `Open ${lead} (+${rest} more plan${rest === 1 ? "" : "s"})`;
}

// Freshest plan's name — the row's primary content, not just a tooltip.
export function workspacePlanEntryPrimaryTitle(entries: PlanIndexEntry[]): string {
  return entries[0]?.title.trim() || "Untitled plan";
}

export function workspacePlanEntryHint(entries: PlanIndexEntry[]): string | null {
  const e = entries[0];
  if (!e) return null;
  const count = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
  const openTasks = typeof e.open_task_count === "number" ? e.open_task_count : (e.task_count ?? 0);
  if (openTasks > 0) return count(openTasks, "task");
  if ((e.section_count ?? 0) > 0) return count(e.section_count as number, "section");
  if ((e.element_count ?? 0) > 0) return count(e.element_count as number, "item");
  return null;
}

export function workspacePlanEntryExtraCount(entries: PlanIndexEntry[]): number {
  return Math.max(0, entries.length - 1);
}

export function resolveTabPlanFocusTarget(
  entries: PlanIndexEntry[],
  tabId: string,
): CanvasWorkspaceTarget | null {
  if (entries.length === 0) return null;
  return {
    focusTabId: tabId,
    planId: entries.length === 1 ? entries[0].plan_id : undefined,
  };
}

export function openWorkspacePlanCanvas(target?: CanvasWorkspaceTarget): void {
  if (target?.planId) canvasFocusStore.clearPulse(target.planId);
  canvasFocusStore.resetWorkspaceDismissed();
  canvasFocusStore.openWorkspace(target ?? undefined);
}

export interface TeamPlanCanvasTarget {
  teamId: string;
  managerTabId: string | null;
  planId?: string;
}

export function openTeamPlanCanvas(target: TeamPlanCanvasTarget): void {
  openWorkspacePlanCanvas({
    teamId: target.teamId,
    managerTabId: target.managerTabId,
    focusTabId: target.managerTabId ?? undefined,
    planId: target.planId,
  });
}

export function openWorkspacePlanForEntries(entries: readonly PlanIndexEntry[]): void {
  openWorkspacePlanCanvas({
    planId: entries.length === 1 ? entries[0].plan_id : undefined,
  });
}

/** Workspace whose cwd owns this plan's canonical project, or null when unknown. */
export function findPlanOwnerWorkspace(
  planId: string,
  index: readonly PlanIndexEntry[],
  workspaces: readonly Workspace[],
  canonicalProjectPath?: string,
): { workspace: Workspace; entry?: PlanIndexEntry } | null {
  const entry = index.find((e) => e.plan_id === planId);
  const authoritativePath = canonicalProjectPath?.trim();
  if (!entry && !authoritativePath) return null;
  let best: { workspace: Workspace; entry?: PlanIndexEntry } | null = null;
  let bestLen = -1;
  for (const workspace of workspaces) {
    const belongs = authoritativePath
      ? workspace.cwd != null && projectPathBelongsToWorkspace(authoritativePath, workspace.cwd)
      : entry != null && planBelongsToWorkspacePath(entry, workspace);
    if (!belongs) continue;
    const len = workspace.cwd ? normalizePath(workspace.cwd).length : 0;
    if (len > bestLen) {
      best = { workspace, entry };
      bestLen = len;
    }
  }
  return best;
}

export interface OpenPlanDeepLinkCtx {
  index: readonly PlanIndexEntry[];
  workspaces: readonly Workspace[];
  activeWorkspaceId: string | null;
  canonicalProjectPath?: string;
  setActiveWorkspace: (id: string) => void;
  onMissing?: (planId: string) => void;
}

/** Deep-link from chat: switch to the owning workspace and open the plan focused on a task. */
export function openPlanDeepLink(planId: string, taskId: string | undefined, ctx: OpenPlanDeepLinkCtx): boolean {
  const owner = findPlanOwnerWorkspace(
    planId,
    ctx.index,
    ctx.workspaces,
    ctx.canonicalProjectPath,
  );
  if (!owner) {
    ctx.onMissing?.(planId);
    return false;
  }
  if (owner.workspace.id !== ctx.activeWorkspaceId) ctx.setActiveWorkspace(owner.workspace.id);
  openWorkspacePlanCanvas({ planId, nodeIds: taskId ? [taskId] : undefined });
  return true;
}

export function resolveWorkspaceFocusNodeIds(
  mode: "workspace" | "focus",
  target: CanvasWorkspaceTarget | null | undefined,
  plans: readonly Plan[],
): string[] {
  if (mode !== "workspace" || !target) return [];
  if (target.nodeIds?.length) return target.nodeIds;
  if (target.focusTabId) {
    const taskIds = assignedTaskIdsForTab(plans, target.focusTabId);
    if (taskIds.length) return taskIds;
  }
  if (target.planId) return [target.planId];
  return [];
}

export function canvasFitContextKey(
  mode: string,
  focusTabId: string | null | undefined,
  workspaceTarget: CanvasWorkspaceTarget | null | undefined,
): string {
  const ws = workspaceTarget;
  return `${mode}:${focusTabId ?? ""}:${ws?.focusTabId ?? ""}:${ws?.planId ?? ""}:${ws?.nodeIds?.join(",") ?? ""}`;
}

export function openTabPlanFocus(
  entries: PlanIndexEntry[],
  tabId: string,
  nodeIds?: string[],
): void {
  const base = resolveTabPlanFocusTarget(entries, tabId);
  if (!base) return;
  openWorkspacePlanCanvas({
    ...base,
    nodeIds: nodeIds?.length ? nodeIds : undefined,
  });
}
