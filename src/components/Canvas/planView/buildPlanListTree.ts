import type { Tab } from "../../../lib/types";
import { resolvePlanExclusiveTabTasks } from "../tabCurrentTask";
import { flattenSections } from "../sectionRows";
import { buildTaskPresence } from "../taskPresence";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import type { Milestone, Plan, Task, TaskStatus } from "../types";
import { canonicalTaskStatusLabel } from "./taskStatusLabel";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { deriveWorkingOnRow } from "./workingOnRow";
import type { CompetitionForTaskResult } from "./competitionForTask";
import type { LoopForTaskResult } from "./loopForTask";
import type { WorkingOnRowModel } from "./workingOnRow";

export type PlanListRowKind = "plan-section" | "unsectioned" | "task" | "subtask";

export interface PlanListRow {
  id: string;
  planId: string;
  kind: PlanListRowKind;
  depth: number;
  title: string;
  status?: TaskStatus;
  statusLabel?: string;
  unplannedLabel?: string;
  taskCount?: number;
  doneCount?: number;
  allTasksDone?: boolean;
  sectionTitle?: string;
  tabLabel?: string;
  historicalExecutorSummary?: string;
  focus?: string;
  pauseReason?: string;
  milestoneKind?: string;
  milestoneAuthor?: string;
  activityTs?: number;
  taskId?: string;
  loopBadge?: LoopForTaskResult;
  competeBadge?: CompetitionForTaskResult;
  workingOn?: WorkingOnRowModel;
}

export type PlanListVisibleRow =
  | { type: "row"; row: PlanListRow }
  | { type: "done-rollup"; sectionRow: PlanListRow; doneCount: number }
  | { type: "done-filter-toggle"; totalCount: number; hiddenCount: number; expanded: boolean };

const SECTION_KINDS = new Set<PlanListRowKind>(["plan-section", "unsectioned"]);
const TASK_KINDS = new Set<PlanListRowKind>(["task", "subtask"]);

export function isPlanListSectionRow(row: PlanListRow): boolean {
  return SECTION_KINDS.has(row.kind);
}

export function isPlanListTaskRow(row: PlanListRow): boolean {
  return TASK_KINDS.has(row.kind);
}

function latestMilestone(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
}

function statusText(task: Task): string {
  return canonicalTaskStatusLabel(task);
}

function sectionTitle(task: Task, sectionById: ReadonlyMap<string, string>): string {
  for (const sid of task.section_ids) {
    const title = sectionById.get(sid.trim());
    if (title) return title;
  }
  return "Unsectioned";
}

function taskKind(task: Task, taskIds: ReadonlySet<string>): "task" | "subtask" {
  return task.parent_task_id && taskIds.has(task.parent_task_id) ? "subtask" : "task";
}

function buildTaskRow(
  task: Task,
  plan: Plan,
  tabs: Record<string, Tab>,
  exclusiveTabTasks: Map<string, string>,
  sectionById: ReadonlyMap<string, string>,
  taskIds: ReadonlySet<string>,
): PlanListRow {
  const presence = buildTaskPresence(plan, task, tabs, exclusiveTabTasks);
  const historical = historicalExecutorForTask(task, tabs, presence != null);
  const latest = latestMilestone(task);
  const kind = taskKind(task, taskIds);
  return {
    id: `${plan.id}::${kind}::${task.id}`,
    planId: plan.id,
    kind,
    depth: kind === "subtask" ? 3 : 2,
    title: task.name,
    status: task.status,
    statusLabel: statusText(task),
    unplannedLabel: task.tags?.includes("unplanned") ? task.name : undefined,
    sectionTitle: sectionTitle(task, sectionById),
    tabLabel: presence?.label ?? undefined,
    historicalExecutorSummary: historical?.summary,
    focus: presence?.focus ?? undefined,
    pauseReason: task.pause_reason?.trim() || undefined,
    milestoneKind: presence?.latest?.kindLabel ?? latest?.kind.replace(/[_-]+/g, " "),
    milestoneAuthor: presence?.latest?.authorLabel,
    activityTs: latest?.ts ?? task.status_changed_at ?? task.updated_at ?? task.created_at,
    taskId: task.id,
    loopBadge: loopForTask(plan, task.id) ?? undefined,
    competeBadge: competitionForTask(task, tabs) ?? undefined,
    workingOn: deriveWorkingOnRow(task, tabs) ?? undefined,
  };
}

export function buildPlanListRows(
  plan: Plan,
  opts: { tabs?: Record<string, Tab> } = {},
): PlanListRow[] {
  const tabs = opts.tabs ?? {};
  const rows: PlanListRow[] = [];
  const exclusiveTabTasks = resolvePlanExclusiveTabTasks(plan);
  const taskIds = new Set(plan.tasks.map((t) => t.id));
  const sectionById = new Map(flattenSections(plan.sections).map((s) => [s.id, s.title]));
  const taskRows = plan.tasks.map((task) => buildTaskRow(task, plan, tabs, exclusiveTabTasks, sectionById, taskIds));
  const groups: Array<{ id: string; title: string; statuses: TaskStatus[] }> = [
    { id: "active", title: "In progress + review", statuses: ["in_progress", "in_review"] },
    { id: "blocked", title: "Blocked", statuses: ["blocked"] },
    { id: "pending", title: "Pending", statuses: ["pending", "paused"] },
    { id: "done", title: "Done", statuses: ["done"] },
    { id: "cancelled", title: "Cancelled", statuses: ["cancelled"] },
  ];

  for (const group of groups) {
    const children = taskRows
      .filter((row) => row.status && group.statuses.includes(row.status))
      .sort((a, b) => (b.activityTs ?? 0) - (a.activityTs ?? 0) || a.title.localeCompare(b.title));
    if (!children.length) continue;
    rows.push({
      id: `${plan.id}::status::${group.id}`,
      planId: plan.id,
      kind: "plan-section",
      depth: 1,
      title: group.title,
      taskCount: children.length,
      doneCount: group.id === "done" ? children.length : undefined,
      allTasksDone: group.id === "done",
    });
    rows.push(...children);
  }

  return rows;
}

export function buildPlanListForest(
  plans: readonly Plan[],
  opts: { tabs?: Record<string, Tab> } = {},
): PlanListRow[] {
  return plans.flatMap((plan) => buildPlanListRows(plan, { tabs: opts.tabs }));
}
