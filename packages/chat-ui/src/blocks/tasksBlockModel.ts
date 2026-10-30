import {
  DS_PLAN_ID_RE,
  DS_TASK_GROUP_ALIASES,
  isDsTaskRef,
  type DsTaskGroupKey,
  type DsTasksBlock,
  type DsTasksBlockItem,
} from "../codeterm/dsTasksBlock";

/** What a `@ds-tasks` block shows: explicit refs and status groups of one plan, or the task set a sent message carried. */
export interface TasksBlockSpec {
  planId: string;
  refs: string[];
  groups: DsTaskGroupKey[];
  /** The task set a `<ds-tasks>` message carried; null when the plan's live tasks decide it. */
  items: DsTasksBlockItem[] | null;
  planTitle?: string;
}

export interface TasksBlockRow {
  id: string;
  key: string;
  name: string;
  status: string;
  color: string | null;
  executor: string | null;
  model: string | null;
  progress: string | null;
  /** The plan no longer has this task; the row shows what the block carried. */
  detached: boolean;
}

export interface TasksBlockGroup {
  key: string;
  label: string;
  rows: TasksBlockRow[];
  collapsedByDefault: boolean;
}

export type TasksBlockState =
  | { kind: "loading" }
  | { kind: "missing" }
  | { kind: "ready"; title: string; total: number; groups: TasksBlockGroup[] };

/** `<plan-id> [task-id|TASK-KEY|status ...]`; any other word makes the line plain text. */
export function parseTasksArgument(argument: string): TasksBlockSpec | null {
  const [planId, ...words] = argument.trim().split(/\s+/);
  if (!planId || !DS_PLAN_ID_RE.test(planId)) return null;
  const spec: TasksBlockSpec = { planId, refs: [], groups: [], items: null };
  for (const word of words) {
    const group = DS_TASK_GROUP_ALIASES[word.toLowerCase()];
    if (isDsTaskRef(word)) {
      if (!spec.refs.includes(word)) spec.refs.push(word);
    } else if (group) {
      if (!spec.groups.includes(group)) spec.groups.push(group);
    } else {
      return null;
    }
  }
  return spec;
}

/** A sent `<ds-tasks>` block keeps its own task set and statuses as the fallback for tasks the plan lost. */
export function tasksSpecFromBlock(block: DsTasksBlock): TasksBlockSpec {
  const groups = block.filter.flatMap((word) => DS_TASK_GROUP_ALIASES[word.toLowerCase()] ?? []);
  return { planId: block.planId, refs: [], groups, items: block.items, planTitle: block.planTitle };
}

/** The one-line form: what Pin stores and Ask quotes. A carried task set pins as its task ids. */
export function tasksBlockSource(spec: TasksBlockSpec): string {
  const refs = spec.items ? spec.items.map((item) => item.id) : spec.refs;
  const words = spec.items ? refs : [...refs, ...spec.groups];
  return ["@ds-tasks", spec.planId, ...words].join(" ");
}

/** The live-binding key; the host decodes it with `decodeTasksBlockKey`. */
export function tasksBlockKey(spec: TasksBlockSpec): string {
  return JSON.stringify([spec.planId, spec.refs, spec.groups, spec.items]);
}

export function decodeTasksBlockKey(key: string): TasksBlockSpec | null {
  try {
    const [planId, refs, groups, items] = JSON.parse(key) as [string, string[], DsTaskGroupKey[], DsTasksBlockItem[] | null];
    return typeof planId === "string" && Array.isArray(refs) && Array.isArray(groups) ? { planId, refs, groups, items: Array.isArray(items) ? items : null } : null;
  } catch {
    return null;
  }
}

export function tasksBlockSummary(state: Extract<TasksBlockState, { kind: "ready" }>): string {
  const counts = state.groups.map((group) => `${group.rows.length} ${group.label.toLowerCase()}`);
  return counts.length ? counts.join(", ") : "No tasks match";
}
