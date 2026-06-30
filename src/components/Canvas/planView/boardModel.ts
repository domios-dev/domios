import type { Pane } from "../../../lib/types";
import { flattenSections } from "../sectionRows";
import { filterPlansForCanvas } from "../planPaneMatch";
import { resolvePlanExclusivePaneTasks } from "../paneCurrentTask";
import { buildTaskPresence } from "../taskPresence";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import { TaskStatus, type Milestone, type Plan, type Section, type Task, type TaskStatus as TaskStatusType } from "../types";

export const BOARD_COLUMN_DEFS = [
  { key: "pending", label: "Pending", statuses: [TaskStatus.Pending] as TaskStatusType[] },
  { key: "in_progress", label: "In progress", statuses: [TaskStatus.InProgress] as TaskStatusType[] },
  { key: "in_review", label: "In review", statuses: [TaskStatus.InReview] as TaskStatusType[] },
  { key: "blocked", label: "Blocked", statuses: [TaskStatus.Blocked] as TaskStatusType[] },
  { key: "done", label: "Done", statuses: [TaskStatus.Done] as TaskStatusType[] },
] as const;

export type BoardColumnKey = (typeof BOARD_COLUMN_DEFS)[number]["key"];

export interface BoardSpecSummary {
  specId: string;
  title: string;
  sectionCount: number;
  leadSection: string | null;
}

export interface BoardPlanSummary {
  title: string;
  ownerLabel: string;
  done: number;
  total: number;
  focus: string | null;
  sectionCount: number;
  goalVector: string | null;
}

export interface BoardTaskCardModel {
  task: Task;
  presenceLabel: string | null;
  historicalExecutorSummary: string | null;
  roleLabel: string | null;
  branch: string | null;
  worktree: string | null;
  workspaceName: string | null;
  latestMilestone: string | null;
  latestMilestoneKind: string | null;
  focusMatch: boolean;
}

export interface BoardColumnModel {
  key: BoardColumnKey;
  label: string;
  cards: BoardTaskCardModel[];
}

export interface BoardPlanModel {
  planId: string;
  spec: BoardSpecSummary | null;
  planSummary: BoardPlanSummary;
  columns: BoardColumnModel[];
  cancelledCount: number;
}

const EMPTY_LABELS = new Set(["", "unknown"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function ownerLabel(plan: Plan, panes: Record<string, Pane>): string {
  const paneId = clean(plan.owner.pane_id);
  if (paneId && panes[paneId]?.title) return panes[paneId].title!.trim();
  return clean(plan.owner.title) ?? (paneId ? paneId.slice(0, 8) : "Unassigned");
}

function latestMilestone(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
}

function taskFreshness(task: Task): number {
  return task.status_changed_at ?? task.updated_at ?? task.created_at ?? 0;
}

function laneBranch(task: Task): string | null {
  return clean(task.lane?.branch);
}

function laneWorktree(task: Task): string | null {
  const path = clean(task.lane?.worktree_path);
  if (!path) return null;
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.length > 2 ? parts.slice(-2).join("/") : path;
}

function laneWorkspace(task: Task): string | null {
  return clean(task.lane?.workspace_name);
}

function focusMatches(plan: Plan, task: Task, paneId: string | null): boolean {
  const focus = clean(plan.heartbeat?.current_focus);
  if (!focus || !paneId) return false;
  const needles = [task.id, task.handle, task.name, `task:${task.id}`]
    .flatMap((v) => {
      const text = clean(v);
      return text ? [text.toLowerCase()] : [];
    });
  const haystack = focus.toLowerCase();
  return needles.some((n) => haystack.includes(n));
}

function buildTaskCard(
  plan: Plan,
  task: Task,
  panes: Record<string, Pane>,
  exclusivePaneTasks: Map<string, string>,
): BoardTaskCardModel {
  const presence = buildTaskPresence(plan, task, panes, exclusivePaneTasks);
  const historical = historicalExecutorForTask(task, panes, presence != null);
  const milestone = latestMilestone(task);
  const paneId = presence?.paneId ?? clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  return {
    task,
    presenceLabel: presence?.label ?? null,
    historicalExecutorSummary: historical?.summary ?? null,
    roleLabel: presence ? clean(task.taken_by_role) : historical?.role ?? clean(task.taken_by_role),
    branch: laneBranch(task),
    worktree: laneWorktree(task),
    workspaceName: laneWorkspace(task),
    latestMilestone: milestone ? clean(milestone.text) : null,
    latestMilestoneKind: milestone ? milestone.kind.replace(/[_-]+/g, " ") : null,
    focusMatch: focusMatches(plan, task, paneId),
  };
}

export function deriveBoardSpecSummary(
  plan: Plan,
  specSections: Section[] | undefined,
): BoardSpecSummary | null {
  const specId = clean(plan.primary_spec_id);
  if (!specId) return null;
  const sections = specSections ?? [];
  const flat = sections.length ? flattenSections(sections) : [];
  const lead = flat[0]?.title?.trim() || null;
  return {
    specId,
    title: lead ? `Spec · ${lead}` : "Specification",
    sectionCount: flat.length,
    leadSection: lead,
  };
}

export function deriveBoardPlanSummary(plan: Plan, panes: Record<string, Pane>): BoardPlanSummary {
  let done = 0;
  let total = 0;
  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) continue;
    total += 1;
    if (task.status === TaskStatus.Done) done += 1;
  }
  return {
    title: plan.title.trim() || "Untitled plan",
    ownerLabel: ownerLabel(plan, panes),
    done,
    total,
    focus: clean(plan.heartbeat?.current_focus),
    sectionCount: flattenSections(plan.sections).length,
    goalVector: clean(plan.goal?.vector),
  };
}

export function groupTasksIntoColumns(
  plan: Plan,
  panes: Record<string, Pane>,
): { columns: BoardColumnModel[]; cancelledCount: number } {
  const exclusivePaneTasks = resolvePlanExclusivePaneTasks(plan);
  const byStatus = new Map<TaskStatusType, Task[]>();
  let cancelledCount = 0;
  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) {
      cancelledCount += 1;
      continue;
    }
    const bucket = byStatus.get(task.status) ?? [];
    bucket.push(task);
    byStatus.set(task.status, bucket);
  }
  const columns: BoardColumnModel[] = BOARD_COLUMN_DEFS.map((def) => {
    const tasks = def.statuses.flatMap((s) => byStatus.get(s) ?? []);
    tasks.sort((a, b) => taskFreshness(b) - taskFreshness(a));
    return {
      key: def.key,
      label: def.label,
      cards: tasks.map((task) => buildTaskCard(plan, task, panes, exclusivePaneTasks)),
    };
  });
  return { columns, cancelledCount };
}

export function buildBoardPlanModel(
  plan: Plan,
  panes: Record<string, Pane>,
  specSections?: Section[],
): BoardPlanModel {
  const { columns, cancelledCount } = groupTasksIntoColumns(plan, panes);
  return {
    planId: plan.id,
    spec: deriveBoardSpecSummary(plan, specSections),
    planSummary: deriveBoardPlanSummary(plan, panes),
    columns,
    cancelledCount,
  };
}

export function buildBoardModels(
  plans: readonly Plan[],
  panes: Record<string, Pane>,
  specSectionsByPlan: ReadonlyMap<string, Section[]> = new Map(),
): BoardPlanModel[] {
  return plans.map((plan) =>
    buildBoardPlanModel(plan, panes, specSectionsByPlan.get(plan.id)),
  );
}

export function filterPlansForBoard(
  plans: readonly Plan[],
  mode: "workspace" | "focus",
  opts: { planId?: string; orchestratorPaneId?: string; panes?: Record<string, Pane> },
): Plan[] {
  return filterPlansForCanvas([...plans], mode, opts);
}
