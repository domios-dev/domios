import type { LaidOutNode } from "../layout";
import type { CanvasElement, DriftItem, Section, TagFacet, Task } from "../types";
import type { TaskPresence } from "../taskPresence";
import type { HistoricalExecutorView } from "../taskExecutorHistory";

export interface TaskCardModel {
  task: Task;
  tagFacets?: TagFacet[];
  childCount: number;
  childDone: number;
  parentTaskName: string | null;
  stepColors: Map<string, string>;
  drift?: DriftItem;
  presence?: TaskPresence | null;
  historicalExecutor?: HistoricalExecutorView | null;
  /** Card surface tint = the task's region (its main section; subtasks inherit parent's). */
  regionColor?: string;
}

// Archived-plan lineage ghost model (relocated from the retired DOM PlanGhostCard).
export interface PlanGhostModel {
  planId: string;
  title: string;
  status: string;
  sectionCount?: number;
  taskCount?: number;
}

export interface CanvasLookups {
  nodeById: Map<string, LaidOutNode>;
  taskById: Map<string, Task>;
  taskModelById: Map<string, TaskCardModel>;
  sectionById: Map<string, Section>;
  milestoneTaskById: Map<string, string>;
  elementById: Map<string, CanvasElement>;
}

export function collectSections(sections: Section[], out: Map<string, Section>): void {
  for (const s of sections) {
    out.set(s.id, s);
    if (s.subsections.length) collectSections(s.subsections, out);
  }
}
