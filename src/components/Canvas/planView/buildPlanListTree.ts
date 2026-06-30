import type { Pane } from "../../../lib/types";
import { resolvePlanExclusivePaneTasks } from "../paneCurrentTask";
import { flattenSections, sectionRows, type SectionRowGlyph } from "../sectionRows";
import { buildTaskPresence } from "../taskPresence";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import type { Milestone, Plan, Section, Task, TaskStatus } from "../types";

export type PlanListRowKind =
  | "spec-root"
  | "spec-section"
  | "plan-root"
  | "plan-section"
  | "unsectioned"
  | "task"
  | "subtask";

export interface PlanListRow {
  id: string;
  planId: string;
  kind: PlanListRowKind;
  depth: number;
  title: string;
  kicker?: string;
  meta?: string;
  status?: TaskStatus | Plan["status"];
  statusLabel?: string;
  sectionGlyph?: SectionRowGlyph;
  taskCount?: number;
  paneLabel?: string;
  historicalExecutorSummary?: string;
  focus?: string;
  milestoneKind?: string;
  milestoneAuthor?: string;
  taskId?: string;
}

function childOrder(children: Task[]): Task[] {
  return [...children].sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id));
}

function latestMilestone(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
}

function statusText(task: Task): string {
  return task.status_label?.trim() || task.status.replace(/_/g, " ");
}

function walkSpecSections(sections: Section[], planId: string, depth: number, out: PlanListRow[]): void {
  for (const s of sections) {
    out.push({
      id: `${planId}::spec-sec::${s.id}`,
      planId,
      kind: "spec-section",
      depth,
      title: s.title,
    });
    if (s.subsections.length) walkSpecSections(s.subsections, planId, depth + 1, out);
  }
}

function appendTaskRows(
  task: Task,
  plan: Plan,
  panes: Record<string, Pane>,
  exclusivePaneTasks: Map<string, string>,
  depth: number,
  kind: "task" | "subtask",
  out: PlanListRow[],
  childrenByParent: Map<string, Task[]>,
): void {
  const presence = buildTaskPresence(plan, task, panes, exclusivePaneTasks);
  const historical = historicalExecutorForTask(task, panes, presence != null);
  const latest = latestMilestone(task);
  out.push({
    id: `${plan.id}::${kind}::${task.id}`,
    planId: plan.id,
    kind,
    depth,
    title: task.name,
    status: task.status,
    statusLabel: statusText(task),
    paneLabel: presence?.label ?? undefined,
    historicalExecutorSummary: historical?.summary,
    focus: presence?.focus ?? undefined,
    milestoneKind: presence?.latest?.kindLabel ?? latest?.kind.replace(/[_-]+/g, " "),
    milestoneAuthor: presence?.latest?.authorLabel,
    taskId: task.id,
  });
  for (const child of childOrder(childrenByParent.get(task.id) ?? [])) {
    appendTaskRows(child, plan, panes, exclusivePaneTasks, depth + 1, "subtask", out, childrenByParent);
  }
}

function primarySectionId(task: Task): string | null {
  return task.section_ids[0]?.trim() || null;
}

export function buildPlanListRows(
  plan: Plan,
  opts: { specSections?: Section[]; panes?: Record<string, Pane> } = {},
): PlanListRow[] {
  const panes = opts.panes ?? {};
  const rows: PlanListRow[] = [];
  const exclusivePaneTasks = resolvePlanExclusivePaneTasks(plan);

  if (plan.primary_spec_id && opts.specSections?.length) {
    rows.push({
      id: `${plan.id}::spec-root`,
      planId: plan.id,
      kind: "spec-root",
      depth: 0,
      title: plan.title,
      kicker: "SPEC",
      meta: plan.primary_spec_id,
    });
    walkSpecSections(opts.specSections, plan.id, 1, rows);
  }

  const ownerLabel =
    (plan.owner.pane_id ? panes[plan.owner.pane_id]?.title?.trim() : null) ||
    plan.owner.title?.trim() ||
    undefined;
  const heartbeatFocus = plan.heartbeat?.current_focus?.trim() || undefined;
  rows.push({
    id: `${plan.id}::plan-root`,
    planId: plan.id,
    kind: "plan-root",
    depth: 0,
    title: plan.title,
    kicker: "PLAN",
    status: plan.status,
    meta: ownerLabel,
    focus: heartbeatFocus,
  });

  const secRows = sectionRows(plan);
  const secRowById = new Map(secRows.map((r) => [r.section.id, r]));
  const taskIds = new Set(plan.tasks.map((t) => t.id));
  const childrenByParent = new Map<string, Task[]>();
  for (const t of plan.tasks) {
    const pid = t.parent_task_id;
    if (pid && taskIds.has(pid)) {
      const arr = childrenByParent.get(pid);
      if (arr) arr.push(t);
      else childrenByParent.set(pid, [t]);
    }
  }
  const topLevel = plan.tasks.filter((t) => !t.parent_task_id || !taskIds.has(t.parent_task_id));
  const bySection = new Map<string, Task[]>();
  const unsectioned: Task[] = [];
  for (const t of topLevel) {
    const sid = primarySectionId(t);
    if (!sid) unsectioned.push(t);
    else {
      const arr = bySection.get(sid);
      if (arr) arr.push(t);
      else bySection.set(sid, [t]);
    }
  }

  const walkPlanSections = (sections: Section[], depth: number): void => {
    for (const s of sections) {
      const sr = secRowById.get(s.id);
      rows.push({
        id: `${plan.id}::plan-sec::${s.id}`,
        planId: plan.id,
        kind: "plan-section",
        depth,
        title: s.title,
        sectionGlyph: sr?.glyph,
        taskCount: sr?.taskCount,
      });
      for (const t of childOrder(bySection.get(s.id) ?? [])) {
        appendTaskRows(t, plan, panes, exclusivePaneTasks, depth + 1, "task", rows, childrenByParent);
      }
      if (s.subsections.length) walkPlanSections(s.subsections, depth + 1);
    }
  };
  walkPlanSections(plan.sections, 1);

  const planSectionIds = new Set(flattenSections(plan.sections).map((s) => s.id));
  for (const sid of [...bySection.keys()].filter((k) => !planSectionIds.has(k)).sort()) {
    rows.push({
      id: `${plan.id}::plan-sec::${sid}`,
      planId: plan.id,
      kind: "plan-section",
      depth: 1,
      title: sid,
      taskCount: bySection.get(sid)?.length,
    });
    for (const t of childOrder(bySection.get(sid) ?? [])) {
      appendTaskRows(t, plan, panes, exclusivePaneTasks, 2, "task", rows, childrenByParent);
    }
  }

  if (unsectioned.length) {
    rows.push({
      id: `${plan.id}::unsectioned`,
      planId: plan.id,
      kind: "unsectioned",
      depth: 1,
      title: "Unsectioned",
      taskCount: unsectioned.length,
    });
    for (const t of childOrder(unsectioned)) {
      appendTaskRows(t, plan, panes, exclusivePaneTasks, 2, "task", rows, childrenByParent);
    }
  }

  return rows;
}

export function buildPlanListForest(
  plans: readonly Plan[],
  opts: { specSectionsByPlan?: ReadonlyMap<string, Section[]>; panes?: Record<string, Pane> } = {},
): PlanListRow[] {
  return plans.flatMap((plan) =>
    buildPlanListRows(plan, {
      specSections: opts.specSectionsByPlan?.get(plan.id),
      panes: opts.panes,
    }),
  );
}
