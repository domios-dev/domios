import { useMemo } from "react";
import type { Tab } from "../../../lib/types";
import type { LaidOutNode, WorkspaceLayout } from "../layout";
import { milestoneColorMap } from "../milestoneColors";
import { zoneColor } from "../layout/sectionZones";
import { resolvePlanExclusiveTabTasks } from "../tabCurrentTask";
import { buildTaskPresence } from "../taskPresence";
import { historicalExecutorForTask } from "../taskExecutorHistory";
import type { CanvasElement, DriftItem, Plan, Section, Task } from "../types";
export { buildTaskPresenceFingerprint } from "./presenceFingerprint";
import { collectSections, type CanvasLookups, type TaskCardModel } from "./models";

export function useCanvasLookups(
  layout: WorkspaceLayout,
  filteredPlans: Plan[],
  driftByPlan: Map<string, DriftItem[]>,
  tabs: Record<string, Tab>,
  // tabs). Used as the rebuild trigger so a non-presence heartbeat is a no-op.
  tabsContentKey: string,
): CanvasLookups {
  return useMemo<CanvasLookups>(() => {
    const nodeById = new Map<string, LaidOutNode>();
    for (const frame of layout.frames) for (const n of frame.nodes) nodeById.set(n.id, n);
    const taskById = new Map<string, Task>();
    const taskModelById = new Map<string, TaskCardModel>();
    const sectionById = new Map<string, Section>();
    const milestoneTaskById = new Map<string, string>();
    const elementById = new Map<string, CanvasElement>();
    for (const plan of filteredPlans) {
      collectSections(plan.sections, sectionById);
      const childCounts = new Map<string, number>();
      const childDone = new Map<string, number>();
      const taskNameById = new Map(plan.tasks.map((t) => [t.id, t.name]));
      for (const t of plan.tasks) {
        if (t.parent_task_id) {
          childCounts.set(t.parent_task_id, (childCounts.get(t.parent_task_id) ?? 0) + 1);
          if (t.status === "done" || t.status === "cancelled") childDone.set(t.parent_task_id, (childDone.get(t.parent_task_id) ?? 0) + 1);
        }
      }
      const colors = milestoneColorMap(plan);
      // own — it inherits the region of the top-level task that spawned it (walk up).
      const taskByIdLocal = new Map(plan.tasks.map((t) => [t.id, t]));
      const regionSectionOf = (t: Task): string | undefined => {
        let cur: Task = t;
        for (let guard = 0; cur.parent_task_id && taskByIdLocal.has(cur.parent_task_id) && guard < 32; guard++) {
          cur = taskByIdLocal.get(cur.parent_task_id)!;
        }
        return cur.section_ids[0];
      };
      const driftForPlan = driftByPlan.get(plan.id) ?? [];
      const driftByTask = new Map<string, DriftItem>();
      for (const item of driftForPlan) {
        const existing = driftByTask.get(item.task_id);
        if (!existing || (existing.not_mine && !item.not_mine)) driftByTask.set(item.task_id, item);
      }
      const exclusiveTabTasks = resolvePlanExclusiveTabTasks(plan);
      for (const task of plan.tasks) {
        taskById.set(task.id, task);
        const parentTaskName = task.parent_task_id ? taskNameById.get(task.parent_task_id) ?? null : null;
        const presence = buildTaskPresence(plan, task, tabs, exclusiveTabTasks);
        taskModelById.set(task.id, {
          task,
          tagFacets: plan.tag_facets,
          childCount: childCounts.get(task.id) ?? 0,
          childDone: childDone.get(task.id) ?? 0,
          parentTaskName,
          stepColors: colors,
          drift: driftByTask.get(task.id),
          presence,
          historicalExecutor: historicalExecutorForTask(task, tabs, presence != null),
          regionColor: (() => { const sid = regionSectionOf(task); return sid ? zoneColor(sid) : undefined; })(),
        });
        for (const m of task.milestones) milestoneTaskById.set(m.id, task.id);
      }
      for (const el of plan.elements) elementById.set(el.id, el);
    }
    return { nodeById, taskById, taskModelById, sectionById, milestoneTaskById, elementById };
    // `tabs` is read inside but intentionally NOT a dep — tabsContentKey is its
    // presence-relevant projection, so heartbeats that don't change it are skipped.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layout, filteredPlans, driftByPlan, tabsContentKey]);
}
