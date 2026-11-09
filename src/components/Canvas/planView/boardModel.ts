import type { PlanView as Plan, PlanTaskView as Task } from "../planProjection";
import type { Tab } from "../../../lib/types";
import { flattenSections } from "../sectionRows";
import { filterPlansForCanvas, planBelongsToWorkspace, type PlanFilterOpts } from "../planTabMatch";
import { resolvePlanExclusiveTabTasks } from "../tabCurrentTask";
import { buildTaskPresence } from "../taskPresence";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import {
  TaskStatus,
  type PlanningState,
  type Section,
  type TaskStatus as TaskStatusType,
  type PlanTaskBoardSummary,
  type PlanTaskCompetitionBadge,
  type PlanTaskHistoricalExecutor,
} from "../types";
import { canonicalTaskStatusLabel, latestTaskNote } from "./taskStatusLabel";
import { qaChipText } from "./taskQa";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { deriveWorkingOnRow } from "./workingOnRow";
import type { CompetitionForTaskResult } from "./competitionForTask";
import type { LoopForTaskResult } from "./loopForTask";
import type { WorkingOnRowModel } from "./workingOnRow";

export const BOARD_COLUMN_DEFS = [
  { key: "pending", label: "Pending", statuses: [TaskStatus.Pending, TaskStatus.Paused] as TaskStatusType[] },
  { key: "in_progress", label: "In progress", statuses: [TaskStatus.InProgress] as TaskStatusType[] },
  { key: "in_review", label: "In review", statuses: [TaskStatus.InReview] as TaskStatusType[] },
  { key: "blocked", label: "Blocked", statuses: [TaskStatus.Blocked] as TaskStatusType[] },
  { key: "done", label: "Done", statuses: [TaskStatus.Done] as TaskStatusType[] },
] as const;

export type BoardColumnKey = (typeof BOARD_COLUMN_DEFS)[number]["key"];

export const DONE_COLUMN_COLLAPSED_LIMIT = 5;

export interface DoneColumnBounds {
  visible: BoardTaskCardModel[];
  total: number;
  hidden: number;
}

export interface BoardSpecSummary {
  specId: string;
  title: string;
  sectionCount: number;
  leadSection: string | null;
}

export interface BoardPlanSummary {
  title: string;
  ownerLabel: string;
  ownerTabId: string | null;
  plannerLabel: string | null;
  plannerTabId: string | null;
  planningState: PlanningState | null;
  executionUnlocked: boolean;
  done: number;
  total: number;
  focus: string | null;
  sectionCount: number;
  goalVector: string | null;
}

/** Where a task lives in the plan: its group (what kind of work) and its
 * lead section (which milestone/phase) — the card's "what and where" locator. */
export interface TaskLocator {
  groupKey: string | null;
  groupLabel: string | null;
  groupColor: string | null;
  section: string | null;
}

export interface BoardTaskCardModel {
  task: BoardTaskCardTask;
  statusChipText: string;
  presenceLabel: string | null;
  historicalExecutorSummary: string | null;
  roleLabel: string | null;
  branch: string | null;
  latestNote: string | null;
  workingOn: WorkingOnRowModel | null;
  loopBadge: LoopForTaskResult | null;
  competeBadge: CompetitionForTaskResult | null;
  focusMatch: boolean;
  locator: TaskLocator | null;
}

/** The renderer's bounded task identity/presentation fields; summaries do not fake full Task records. */
export interface BoardTaskCardTask {
  id: string;
  name: string;
  status: TaskStatusType;
  key?: string | null;
  handle?: string | null;
  pause_reason?: string | null;
  created_at?: number;
  updated_at?: number;
  status_changed_at?: number;
  activity_at?: number;
  assigned_tab_id?: string | null;
  lane?: Pick<NonNullable<Task["lane"]>, "tab_id" | "branch">;
  milestones?: Task["milestones"];
  taken_by_role?: string | null;
  model?: string | null;
  peer?: string | null;
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

function ownerLabel(plan: Plan, tabs: Record<string, Tab>): string {
  const tabId = clean(plan.owner.tab_id);
  if (tabId && tabs[tabId]?.title) return tabs[tabId].title!.trim();
  return clean(plan.owner.title) ?? (tabId ? tabId.slice(0, 8) : "Unassigned");
}

function plannerBinding(
  plan: Plan,
  tabs: Record<string, Tab>,
): { label: string | null; tabId: string | null } {
  const ref = plan.planner ?? null;
  if (!ref) return { label: null, tabId: null };
  if (ref.kind === "waived") return { label: "Solo (waived)", tabId: null };
  const tabId = clean(ref.tab_id);
  const localTitle = tabId ? tabs[tabId]?.title?.trim() : undefined;
  if (localTitle) return { label: localTitle, tabId };
  const refTitle = clean(ref.title);
  if (refTitle) return { label: refTitle, tabId };
  if (tabId) return { label: `${tabId.slice(0, 8)} (remote)`, tabId };
  return { label: null, tabId: null };
}

export function resolveDisplayPlanningState(plan: Plan): PlanningState | null {
  const raw = plan.planning_state;
  if (plan.mode === "complete") {
    if (raw === "approved" || raw === "structure_ready") return raw;
    return null;
  }
  if (plan.mode === "approved" || plan.mode === "execution") {
    if (!raw || raw === "drafting") return "approved";
    return raw;
  }
  return raw ?? null;
}

export const PLANNING_STATE_LABEL: Record<PlanningState, string> = {
  drafting: "Drafting",
  structure_ready: "Structure ready",
  approved: "Approved",
};

export type PlanDraftState = Exclude<PlanningState, "approved">;

/** Non-null while the plan still waits for approval; approval through setPlanMode clears it. */
export function planDraftState(plan: Pick<Plan, "mode" | "planning_state">): PlanDraftState | null {
  const state = plan.mode === "planning" ? plan.planning_state ?? "drafting" : plan.mode === "paused" ? plan.planning_state : null;
  return state && state !== "approved" ? state : null;
}

// approved/execution lifecycle mode bridges plans predating planning_state.
export function planExecutionUnlocked(plan: Plan): boolean {
  if (plan.mode === "complete") return false;
  return plan.planning_state === "approved" || plan.mode === "approved" || plan.mode === "execution";
}

function laneBranch(task: Task): string | null {
  return clean(task.lane?.branch);
}

function focusMatches(plan: Plan, task: Task, tabId: string | null): boolean {
  const focus = clean(plan.heartbeat?.current_focus);
  if (!focus || !tabId) return false;
  const needles = [task.id, task.handle, task.name, `task:${task.id}`]
    .flatMap((v) => {
      const text = clean(v);
      return text ? [text.toLowerCase()] : [];
    });
  const haystack = focus.toLowerCase();
  return needles.some((n) => haystack.includes(n));
}

function buildTaskLocator(plan: Plan, task: Task): TaskLocator | null {
  const groupKey = clean(task.group);
  const group = groupKey ? plan.task_groups?.find((g) => g.key === groupKey) ?? null : null;
  const sectionId = task.section_ids?.[0];
  let section: string | null = null;
  if (sectionId) {
    const flat = flattenSections(plan.sections);
    section = flat.find((s) => s.id === sectionId)?.title?.trim() || null;
  }
  if (!groupKey && !section) return null;
  return {
    groupKey,
    groupLabel: group?.label?.trim() || groupKey,
    groupColor: clean(group?.color),
    section,
  };
}

function buildTaskCard(
  plan: Plan,
  task: Task,
  tabs: Record<string, Tab>,
  exclusiveTabTasks: Map<string, string>,
  now: number,
): BoardTaskCardModel {
  const presence = buildTaskPresence(plan, task, tabs, exclusiveTabTasks);
  const historical = historicalExecutorForTask(task, tabs, presence != null);
  const tabId = presence?.tabId ?? clean(task.assigned_tab_id) ?? clean(task.lane?.tab_id);
  return {
    task,
    statusChipText: qaChipText(plan, task) ?? canonicalTaskStatusLabel(task, plan),
    presenceLabel: presence?.label ?? null,
    historicalExecutorSummary: historical?.summary ?? null,
    roleLabel: presence ? clean(task.taken_by_role) : historical?.role ?? clean(task.taken_by_role),
    branch: laneBranch(task),
    latestNote: latestTaskNote(task),
    workingOn: deriveWorkingOnRow(task, tabs, now),
    loopBadge: loopForTask(plan, task.id),
    competeBadge: competitionForTask(task, tabs),
    focusMatch: focusMatches(plan, task, tabId),
    locator: buildTaskLocator(plan, task),
  };
}

function executorSummary(
  summary: PlanTaskHistoricalExecutor | null | undefined,
  task: BoardTaskCardTask,
  tabs: Record<string, Tab>,
): string | null {
  if (task.status !== TaskStatus.Done && task.status !== TaskStatus.Cancelled && task.status !== TaskStatus.Paused) return null;
  const tabId = summary?.tab_id?.trim() || task.assigned_tab_id?.trim() || task.lane?.tab_id?.trim() || null;
  const tabTitle = summary?.tab_title?.trim() || (tabId ? tabs[tabId]?.title?.trim() : null);
  const label = tabTitle || (tabId ? (tabId.length > 12 ? tabId.slice(0, 8) : tabId) : null);
  const fields = [
    label,
    summary?.role?.trim() || task.taken_by_role?.trim(),
    summary?.model?.trim(),
    summary?.branch?.trim() || task.lane?.branch?.trim(),
    summary?.report_status?.trim() || summary?.outcome?.trim(),
  ].filter((value): value is string => Boolean(value));
  return fields.length ? fields.join(" · ") : null;
}

function competitionBadge(
  summary: PlanTaskCompetitionBadge | null | undefined,
  tabs: Record<string, Tab>,
): CompetitionForTaskResult | null {
  if (!summary) return null;
  const lines = [`${summary.count} competitive attempts configured`];
  if (summary.models.length) lines.push(`models: ${summary.models.join(", ")}`);
  if (summary.judge_hint?.trim()) lines.push(`judge: ${summary.judge_hint.trim()}`);
  if (summary.live_count === 0) {
    lines.push("no live attempts");
  } else {
    for (const attempt of summary.live_attempts) {
      const id = attempt.tab_id?.trim();
      const label = attempt.tab_title?.trim() || (id ? tabs[id]?.title?.trim() : null) || (id ? (id.length > 8 ? id.slice(0, 8) : id) : "unknown");
      const model = attempt.model?.trim();
      const lane = attempt.competition_lane != null ? ` lane ${attempt.competition_lane}` : "";
      lines.push(`live: ${label}${lane}${model ? ` · ${model}` : ""}`);
    }
    if (summary.live_count > summary.live_attempts.length) {
      lines.push(`${summary.live_count - summary.live_attempts.length} more live attempts`);
    }
  }
  return { count: summary.count, liveCount: summary.live_count, tooltip: lines.join("\n") };
}

function summaryStatusLabel(summary: PlanTaskBoardSummary): string {
  if (summary.status === TaskStatus.InReview && summary.review_stage === "qa") {
    if (summary.qa_state === "running") return "In QA · running";
    const position = summary.qa_queue_position ?? 0;
    const base = position > 0 ? `In QA · queued #${position}` : "In QA · queued";
    const reason = summary.status_label?.trim();
    return reason ? `${base} — ${reason}` : base;
  }
  return summary.status.replace(/_/g, " ");
}

function summaryLatestNote(text: string | null | undefined): string | null {
  const normalized = text?.replace(/\s+/g, " ").trim();
  if (!normalized) return null;
  return normalized.length <= 120 ? normalized : `${normalized.slice(0, 119)}…`;
}

export function buildSummaryBoardTaskCard(
  summary: PlanTaskBoardSummary,
  tabs: Record<string, Tab>,
  now: number = Date.now(),
): BoardTaskCardModel {
  const task: BoardTaskCardTask = {
    id: summary.id,
    name: summary.name,
    status: summary.status,
    key: summary.key,
    handle: summary.handle,
    pause_reason: summary.pause_reason,
    created_at: summary.created_at,
    updated_at: summary.updated_at,
    status_changed_at: summary.status_changed_at,
    activity_at: summary.activity_at ?? Math.max(summary.created_at, summary.updated_at, summary.status_changed_at),
    assigned_tab_id: summary.assigned_tab_id,
    lane: summary.lane,
    taken_by_role: summary.taken_by_role,
    model: summary.model,
    peer: summary.peer,
  };
  const tabId = task.assigned_tab_id?.trim() || task.lane?.tab_id?.trim() || null;
  const canHavePresence = task.status !== TaskStatus.Done && task.status !== TaskStatus.Cancelled && task.status !== TaskStatus.Paused;
  const presenceLabel = canHavePresence && tabId && tabs[tabId]
    ? tabs[tabId]?.title?.trim() || tabId.slice(0, 8)
    : null;
  const locator = summary.locator;
  return {
    task,
    statusChipText: summaryStatusLabel(summary),
    presenceLabel,
    historicalExecutorSummary: executorSummary(summary.historical_executor, task, tabs),
    roleLabel: summary.historical_executor?.role?.trim() || task.taken_by_role?.trim() || null,
    branch: task.lane?.branch?.trim() || null,
    latestNote: summaryLatestNote(summary.latest_note),
    workingOn: deriveWorkingOnRow(task, tabs, now),
    loopBadge: summary.loop_badge ?? null,
    competeBadge: competitionBadge(summary.competition_badge, tabs),
    focusMatch: false,
    locator: locator && (locator.groupKey || locator.section)
      ? {
          groupKey: locator.groupKey,
          groupLabel: locator.groupLabel?.trim() || locator.groupKey,
          groupColor: locator.groupColor,
          section: locator.section,
        }
      : null,
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

export function deriveBoardPlanSummary(plan: Plan, tabs: Record<string, Tab>): BoardPlanSummary {
  let done = 0;
  let total = 0;
  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) continue;
    total += 1;
    if (task.status === TaskStatus.Done) done += 1;
  }
  if (plan.task_counts) {
    done = plan.task_counts.done ?? 0;
    total = plan.task_counts.total - (plan.task_counts.cancelled ?? 0);
  }
  const planner = plannerBinding(plan, tabs);
  return {
    title: plan.title.trim() || "Untitled plan",
    ownerLabel: ownerLabel(plan, tabs),
    ownerTabId: clean(plan.owner.tab_id),
    plannerLabel: planner.label,
    plannerTabId: planner.tabId,
    planningState: resolveDisplayPlanningState(plan),
    executionUnlocked: planExecutionUnlocked(plan),
    done,
    total,
    focus: clean(plan.heartbeat?.current_focus),
    sectionCount: flattenSections(plan.sections).length,
    goalVector: clean(plan.goal?.vector),
  };
}

export function groupTasksIntoColumns(
  plan: Plan,
  tabs: Record<string, Tab>,
  now: number = Date.now(),
): { columns: BoardColumnModel[]; cancelledCount: number } {
  const exclusiveTabTasks = resolvePlanExclusiveTabTasks(plan);
  const planPosition = new Map(plan.tasks.map((task, index) => [task.id, index]));
  let cancelledCount = 0;
  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) {
      cancelledCount += 1;
    }
  }
  const columns: BoardColumnModel[] = BOARD_COLUMN_DEFS.map((def) => {
    const statuses = new Set<TaskStatusType>(def.statuses);
    const tasks = plan.tasks.filter((task) => statuses.has(task.status));
    tasks.sort((a, b) => {
      const createdA = a.created_at ?? Number.POSITIVE_INFINITY;
      const createdB = b.created_at ?? Number.POSITIVE_INFINITY;
      return createdA - createdB || planPosition.get(a.id)! - planPosition.get(b.id)!;
    });
    return {
      key: def.key,
      label: def.label,
      cards: tasks.map((task) => buildTaskCard(plan, task, tabs, exclusiveTabTasks, now)),
    };
  });
  return { columns, cancelledCount };
}

export function buildBoardPlanModel(
  plan: Plan,
  tabs: Record<string, Tab>,
  specSections?: Section[],
  now: number = Date.now(),
): BoardPlanModel {
  const { columns, cancelledCount } = groupTasksIntoColumns(plan, tabs, now);
  return {
    planId: plan.id,
    spec: deriveBoardSpecSummary(plan, specSections),
    planSummary: deriveBoardPlanSummary(plan, tabs),
    columns,
    cancelledCount,
  };
}

/** Latest activity timestamp across the plan — plan/task stamps + milestone ts. */
export function planActivityFreshness(plan: Plan): number {
  let max = Math.max(plan.updated_at ?? 0, plan.created_at ?? 0);
  for (const task of plan.tasks) {
    max = Math.max(max, task.status_changed_at ?? 0, task.updated_at ?? 0, task.created_at ?? 0);
    for (const m of task.milestones ?? []) max = Math.max(max, m.ts ?? 0);
  }
  return max;
}

/** Freshest plan first; deterministic title→id tiebreak for equal timestamps. */
export function orderPlansByActivity(plans: readonly Plan[]): Plan[] {
  return [...plans].sort((a, b) => {
    const diff = planActivityFreshness(b) - planActivityFreshness(a);
    if (diff !== 0) return diff;
    const ta = a.title.trim().toLowerCase();
    const tb = b.title.trim().toLowerCase();
    if (ta !== tb) return ta < tb ? -1 : 1;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
}

/** Default board plan: explicit planId → orchestrator-owned freshest → freshest
 *  workspace-bound. No workspace-bound plan means no default (empty state), never
 *  a plan from another workspace. */
export function selectDefaultPlanId(
  orderedPlans: readonly Plan[],
  opts: PlanFilterOpts,
): string | null {
  if (!orderedPlans.length) return null;
  if (opts.planId && orderedPlans.some((p) => p.id === opts.planId)) return opts.planId;
  if (opts.orchestratorTabId && opts.tabs) {
    const owned = filterPlansForCanvas([...orderedPlans], "focus", {
      orchestratorTabId: opts.orchestratorTabId,
      tabs: opts.tabs,
      workspaceId: undefined,
    });
    const ownedIds = new Set(owned.map((p) => p.id));
    const first = orderedPlans.find((p) => ownedIds.has(p.id));
    if (first) return first.id;
  }
  // workspaceId undefined = unscoped focus/companion context. Workspace mode scopes upstream in filterPlansForCanvas
  // (empty when unscoped), so no plan from another workspace can reach this fallback.
  const scoped = opts.workspaceId
    ? orderedPlans.filter((p) => planBelongsToWorkspace(p, opts.workspaceId!, opts))
    : orderedPlans;
  if (!scoped.length) return null;
  // Prefer a plan with actionable (non-terminal) work, then any non-empty plan,
  // then the freshest within the workspace scope.
  const hasActionable = (p: Plan) =>
    p.tasks.some((t) => t.status !== TaskStatus.Done && t.status !== TaskStatus.Cancelled);
  const hasWork = (p: Plan) => p.tasks.some((t) => t.status !== TaskStatus.Cancelled);
  return (scoped.find(hasActionable) ?? scoped.find(hasWork) ?? scoped[0]).id;
}

export function buildBoardModels(
  plans: readonly Plan[],
  tabs: Record<string, Tab>,
  specSectionsByPlan: ReadonlyMap<string, Section[]> = new Map(),
  now: number = Date.now(),
): BoardPlanModel[] {
  return plans.map((plan) =>
    buildBoardPlanModel(plan, tabs, specSectionsByPlan.get(plan.id), now),
  );
}

export function boundDoneColumnCards(
  cards: readonly BoardTaskCardModel[],
  expanded: boolean,
  limit: number = DONE_COLUMN_COLLAPSED_LIMIT,
): DoneColumnBounds {
  const total = cards.length;
  if (expanded || total <= limit) {
    return { visible: [...cards], total, hidden: 0 };
  }
  return { visible: cards.slice(0, limit), total, hidden: total - limit };
}

export function filterPlansForBoard(
  plans: readonly Plan[],
  mode: "workspace" | "focus",
  opts: PlanFilterOpts,
): Plan[] {
  return filterPlansForCanvas([...plans], mode, opts);
}
