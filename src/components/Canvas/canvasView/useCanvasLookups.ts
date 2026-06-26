import { useMemo } from "react";
import type { Pane } from "../../../lib/types";
import type { LaidOutNode, WorkspaceLayout } from "../layout";
import type { MilestoneBubbleModel } from "../cards";
import { milestoneColorMap } from "../milestoneColors";
import { sectionRows } from "../sectionRows";
import { buildTaskPresence } from "../taskPresence";
import type { CanvasElement, DriftItem, Plan, Section, Task } from "../types";
import { collectSections, type CanvasLookups, type ClusterCaptionModel, type SpecSectionModel, type TaskCardModel } from "./models";

// Per-data lookups: id→node/task/model/section/milestone/cluster/element maps. Built
// once per layout / plan / drift / panes change — NOT per pan/zoom frame (camera-
// decoupled). Mirrors the original CanvasViewInner useMemo verbatim.
export function useCanvasLookups(
  layout: WorkspaceLayout,
  filteredPlans: Plan[],
  driftByPlan: Map<string, DriftItem[]>,
  panes: Record<string, Pane>,
): CanvasLookups {
  return useMemo<CanvasLookups>(() => {
    const nodeById = new Map<string, LaidOutNode>();
    for (const frame of layout.frames) for (const n of frame.nodes) nodeById.set(n.id, n);
    const taskById = new Map<string, Task>();
    const taskModelById = new Map<string, TaskCardModel>();
    const sectionById = new Map<string, Section>();
    const specSectionById = new Map<string, SpecSectionModel>();
    const milestoneModelById = new Map<string, MilestoneBubbleModel>();
    const milestoneTaskById = new Map<string, string>();
    const clusterModelById = new Map<string, ClusterCaptionModel>();
    const elementById = new Map<string, CanvasElement>();
    for (const plan of filteredPlans) {
      collectSections(plan.sections, sectionById);
      for (const row of sectionRows(plan)) {
        specSectionById.set(`${plan.id}::spec::${row.section.id}`, {
          section: row.section,
          glyph: row.glyph,
          taskCount: row.taskCount,
        });
      }
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
      const driftForPlan = driftByPlan.get(plan.id) ?? [];
      const driftByTask = new Map<string, DriftItem>();
      for (const item of driftForPlan) {
        const existing = driftByTask.get(item.task_id);
        if (!existing || (existing.not_mine && !item.not_mine)) driftByTask.set(item.task_id, item);
      }
      for (const task of plan.tasks) {
        taskById.set(task.id, task);
        const parentTaskName = task.parent_task_id ? taskNameById.get(task.parent_task_id) ?? null : null;
        taskModelById.set(task.id, {
          task,
          tagFacets: plan.tag_facets,
          childCount: childCounts.get(task.id) ?? 0,
          childDone: childDone.get(task.id) ?? 0,
          parentTaskName,
          stepColors: colors,
          drift: driftByTask.get(task.id),
          presence: buildTaskPresence(plan, task, panes),
        });
        const seen = new Map<string, string>();
        for (const m of task.milestones) {
          const color = colors.get(m.kind) ?? "var(--ct-border-focus)";
          milestoneModelById.set(m.id, { milestone: m, color });
          milestoneTaskById.set(m.id, task.id);
          if (!seen.has(m.kind)) seen.set(m.kind, color);
        }
        if (seen.size) {
          clusterModelById.set(`${task.id}::cluster`, {
            kinds: [...seen.entries()].map(([label, color]) => ({ label: label.replace(/_/g, " "), color })),
          });
        }
      }
      for (const el of plan.elements) elementById.set(el.id, el);
    }
    return { nodeById, taskById, taskModelById, sectionById, specSectionById, milestoneModelById, milestoneTaskById, clusterModelById, elementById };
  }, [layout, filteredPlans, driftByPlan, panes]);
}
