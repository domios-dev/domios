import {
  DS_PLAN_ID_RE,
  DS_TASK_GROUP_ALIASES,
  formatDsTasksBlock,
  isDsTaskRef,
  type DsTaskGroupKey,
  type DsTasksBlock,
} from "@codeterm/chat-ui/dsTasksBlock";
import type { Team, Workspace } from "../../../lib/types";
import { planBelongsToWorkspace } from "../../Canvas/planTabMatch";
import { attemptTabTitle } from "../../Canvas/taskExecutorHistory";
import { findTaskByReference, taskDisplayKey } from "../../Canvas/taskDisplayKey";
import type { Plan, Task, TaskExecutionAttempt } from "../../Canvas/types";

export const DS_TASKS_MENTION = "@ds-tasks";

export type { DsTaskGroupKey };

export const DS_TASK_GROUP_ORDER: readonly DsTaskGroupKey[] = [
  "in_progress",
  "in_review",
  "blocked",
  "todo",
  "done",
  "cancelled",
];

const GROUP_LABELS: Record<DsTaskGroupKey, string> = {
  in_progress: "In progress",
  in_review: "In review",
  blocked: "Blocked",
  todo: "To do",
  done: "Done",
  cancelled: "Cancelled",
};

const COLLAPSED_GROUPS = new Set<DsTaskGroupKey>(["done", "cancelled"]);

const MENTION_RE = /(^|\s)@ds-tasks(?=\s|$)/;

export interface DsTasksArgs {
  planId: string | null;
  taskRefs: string[];
  groups: DsTaskGroupKey[];
}

export interface DsTasksMention {
  start: number;
  end: number;
  args: DsTasksArgs;
}

export function groupForStatus(status: string): DsTaskGroupKey {
  switch (status) {
    case "in_progress": return "in_progress";
    case "in_review": return "in_review";
    case "blocked": return "blocked";
    case "done": return "done";
    case "cancelled": return "cancelled";
    default: return "todo";
  }
}

/** Arguments are the recognised tokens right after the mention on its line; the first other word ends them. */
export function parseDsTasksMention(text: string): DsTasksMention | null {
  const match = MENTION_RE.exec(text);
  if (!match) return null;
  const start = match.index + match[1].length;
  let end = start + DS_TASKS_MENTION.length;
  const args: DsTasksArgs = { planId: null, taskRefs: [], groups: [] };
  const tokenRe = /[ \t]+(\S+)/y;
  tokenRe.lastIndex = end;
  for (let token = tokenRe.exec(text); token; token = tokenRe.exec(text)) {
    const word = token[1];
    const group = DS_TASK_GROUP_ALIASES[word.toLowerCase()];
    if (DS_PLAN_ID_RE.test(word) && !args.planId) args.planId = word;
    else if (isDsTaskRef(word)) args.taskRefs.push(word);
    else if (group) {
      if (!args.groups.includes(group)) args.groups.push(group);
    } else break;
    end = tokenRe.lastIndex;
  }
  return { start, end, args };
}

export interface DsTasksScope {
  teamId: string | null;
  workspaceId: string | null;
  teams: readonly Team[];
  workspaces: readonly Workspace[];
}

function newest(plans: readonly Plan[]): Plan | null {
  return plans.reduce<Plan | null>((best, plan) => (!best || plan.updated_at > best.updated_at ? plan : best), null);
}

/** Explicit plan id, else the plan bound to the tab's Team, else the workspace's active plan. */
export function resolveDsTasksPlan(plans: readonly Plan[], planId: string | null, scope: DsTasksScope): Plan | null {
  if (planId) return plans.find((plan) => plan.id === planId) ?? null;
  const active = plans.filter((plan) => plan.status === "active");
  if (scope.teamId) {
    const teamPlan = newest(active.filter((plan) => plan.team_id === scope.teamId));
    if (teamPlan) return teamPlan;
  }
  if (!scope.workspaceId) return null;
  const ctx = { teams: [...scope.teams], workspaces: [...scope.workspaces] };
  return newest(active.filter((plan) => planBelongsToWorkspace(plan, scope.workspaceId!, ctx)));
}

export type DsTasksSelection =
  | { ok: true; tasks: Task[] }
  | { ok: false; missing: string[] };

export function compareTaskKeys(a: { id: string; key?: string | null }, b: { id: string; key?: string | null }): number {
  return taskDisplayKey(a).localeCompare(taskDisplayKey(b), undefined, { numeric: true, sensitivity: "base" });
}

function compareByGroupThenKey(a: Task, b: Task): number {
  const order = DS_TASK_GROUP_ORDER.indexOf(groupForStatus(a.status)) - DS_TASK_GROUP_ORDER.indexOf(groupForStatus(b.status));
  return order !== 0 ? order : compareTaskKeys(a, b);
}

/** Explicit refs and status groups combine as a union; with neither, every task except cancelled ones. */
export function selectDsTasks(plan: Plan, args: Pick<DsTasksArgs, "taskRefs" | "groups">): DsTasksSelection {
  const picked = new Map<string, Task>();
  const missing: string[] = [];
  for (const ref of args.taskRefs) {
    const task = findTaskByReference(plan.tasks, ref);
    if (task) picked.set(task.id, task);
    else missing.push(ref);
  }
  if (missing.length > 0) return { ok: false, missing };
  const groups = new Set(args.groups);
  const byGroup = args.taskRefs.length === 0 && groups.size === 0
    ? plan.tasks.filter((task) => task.status !== "cancelled")
    : plan.tasks.filter((task) => groups.has(groupForStatus(task.status)));
  for (const task of byGroup) picked.set(task.id, task);
  return { ok: true, tasks: [...picked.values()].sort(compareByGroupThenKey) };
}

export function buildDsTasksBlock(plan: Plan, tasks: readonly Task[], args: Pick<DsTasksArgs, "groups">): DsTasksBlock {
  return {
    planId: plan.id,
    planTitle: plan.title,
    filter: [...args.groups],
    items: tasks.map((task) => ({ id: task.id, key: taskDisplayKey(task), status: task.status, name: task.name })),
  };
}

export type DsTasksExpansion =
  | { kind: "none" }
  | { kind: "expanded"; text: string }
  | { kind: "error"; message: string };

/** Replaces the mention with the compact `<ds-tasks>` text the agent receives. */
export function expandDsTasksMention(text: string, plans: readonly Plan[], scope: DsTasksScope): DsTasksExpansion {
  const mention = parseDsTasksMention(text);
  if (!mention) return { kind: "none" };
  const plan = resolveDsTasksPlan(plans, mention.args.planId, scope);
  if (!plan) {
    return {
      kind: "error",
      message: mention.args.planId
        ? `${DS_TASKS_MENTION}: plan ${mention.args.planId} was not found.`
        : `${DS_TASKS_MENTION}: this tab has no Team plan and the workspace has no active plan.`,
    };
  }
  const selection = selectDsTasks(plan, mention.args);
  if (!selection.ok) {
    return { kind: "error", message: `${DS_TASKS_MENTION}: ${selection.missing.join(", ")} not found in ${plan.title}.` };
  }
  const block = formatDsTasksBlock(buildDsTasksBlock(plan, selection.tasks, mention.args));
  const before = text.slice(0, mention.start).trimEnd();
  const after = text.slice(mention.end).trimStart();
  return { kind: "expanded", text: [before, block, after].filter(Boolean).join("\n") };
}

export interface DsTaskRow {
  id: string;
  key: string;
  name: string;
  status: string;
  executor: string | null;
  model: string | null;
  strides: number;
  criteria: number;
  /** The task is no longer in the plan data; the row shows what the message carried. */
  detached: boolean;
}

export interface DsTaskGroup {
  key: DsTaskGroupKey;
  label: string;
  rows: DsTaskRow[];
  collapsedByDefault: boolean;
}

function latestAttempt(task: Task): TaskExecutionAttempt | null {
  return (task.execution_attempts ?? []).reduce<TaskExecutionAttempt | null>(
    (best, attempt) => (!best || (attempt.assigned_at ?? 0) >= (best.assigned_at ?? 0) ? attempt : best),
    null,
  );
}

function clean(value: string | null | undefined): string | null {
  const text = value?.trim();
  return text ? text : null;
}

export function dsTaskRow(task: Task): DsTaskRow {
  const attempt = latestAttempt(task);
  return {
    id: task.id,
    key: taskDisplayKey(task),
    name: task.name,
    status: task.status,
    executor: attempt ? attemptTabTitle(attempt) : null,
    model: clean(attempt?.session_model) ?? clean(attempt?.model) ?? clean(task.model),
    strides: task.milestones.length,
    criteria: task.acceptance_criteria?.length ?? 0,
    detached: false,
  };
}

/** Rows keep the message's task set; status, executor and progress come live from the plan when it has the task. */
export function dsTaskRowsForBlock(block: DsTasksBlock, plan: Plan | null | undefined): DsTaskRow[] {
  const live = new Map((plan?.tasks ?? []).map((task) => [task.id, task]));
  return block.items.map((item) => {
    const task = live.get(item.id);
    if (task) return dsTaskRow(task);
    return {
      id: item.id,
      key: item.key,
      name: item.name,
      status: item.status,
      executor: null,
      model: null,
      strides: 0,
      criteria: 0,
      detached: true,
    };
  });
}

export function groupDsTaskRows(rows: readonly DsTaskRow[]): DsTaskGroup[] {
  const buckets = new Map<DsTaskGroupKey, DsTaskRow[]>();
  for (const row of rows) {
    const key = groupForStatus(row.status);
    const bucket = buckets.get(key);
    if (bucket) bucket.push(row);
    else buckets.set(key, [row]);
  }
  return DS_TASK_GROUP_ORDER.flatMap((key) => {
    const bucket = buckets.get(key);
    if (!bucket) return [];
    return [{
      key,
      label: GROUP_LABELS[key],
      rows: [...bucket].sort(compareTaskKeys),
      collapsedByDefault: COLLAPSED_GROUPS.has(key),
    }];
  });
}

export function dsTaskProgressLabel(row: Pick<DsTaskRow, "strides" | "criteria">): string {
  const strides = `${row.strides} ${row.strides === 1 ? "stride" : "strides"}`;
  if (row.criteria === 0) return strides;
  return `${strides}, ${row.criteria} ${row.criteria === 1 ? "criterion" : "criteria"}`;
}

export interface DsTasksSpec {
  planId: string;
  refs: readonly string[];
  groups: readonly DsTaskGroupKey[];
  items: DsTasksBlock["items"] | null;
}

function detachedRow(ref: string): DsTaskRow {
  return { id: ref, key: ref, name: "", status: "unknown", executor: null, model: null, strides: 0, criteria: 0, detached: true };
}

/** A carried task set keeps its rows; otherwise refs and groups select live, and a ref the plan lost stays as a detached row. */
export function dsTaskRowsForSpec(spec: DsTasksSpec, plan: Plan): DsTaskRow[] {
  if (spec.items) return dsTaskRowsForBlock({ planId: spec.planId, planTitle: plan.title, filter: [...spec.groups], items: spec.items }, plan);
  const rows = new Map<string, DsTaskRow>();
  for (const ref of spec.refs) {
    const task = findTaskByReference(plan.tasks, ref);
    rows.set(task?.id ?? ref, task ? dsTaskRow(task) : detachedRow(ref));
  }
  if (spec.refs.length === 0 || spec.groups.length > 0) {
    const selection = selectDsTasks(plan, { taskRefs: [], groups: [...spec.groups] });
    if (selection.ok) for (const task of selection.tasks) rows.set(task.id, dsTaskRow(task));
  }
  return [...rows.values()];
}
