import type { CanvasLod, MilestoneBubbleModel } from "../cards";
import type { LaidOutNode } from "../layout";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import type { SectionRowGlyph } from "../sectionRows";
import type { CanvasElement, DriftItem, Section, TagFacet, Task } from "../types";
import type { TaskPresence } from "../taskPresence";

export interface ClusterCaptionModel {
  kinds: { label: string; color: string }[];
}

export interface SpecSectionModel {
  section: Section;
  glyph: SectionRowGlyph;
  taskCount: number;
}

export interface TaskCardModel {
  task: Task;
  tagFacets?: TagFacet[];
  childCount: number;
  childDone: number;
  parentTaskName: string | null;
  stepColors: Map<string, string>;
  drift?: DriftItem;
  presence?: TaskPresence | null;
}

export interface CanvasLookups {
  nodeById: Map<string, LaidOutNode>;
  taskById: Map<string, Task>;
  taskModelById: Map<string, TaskCardModel>;
  sectionById: Map<string, Section>;
  specSectionById: Map<string, SpecSectionModel>;
  milestoneModelById: Map<string, MilestoneBubbleModel>;
  milestoneTaskById: Map<string, string>;
  clusterModelById: Map<string, ClusterCaptionModel>;
  elementById: Map<string, CanvasElement>;
}

export function collectSections(sections: Section[], out: Map<string, Section>): void {
  for (const s of sections) {
    out.set(s.id, s);
    if (s.subsections.length) collectSections(s.subsections, out);
  }
}

export function lodForScale(scale: number): CanvasLod {
  const s = DEFAULT_CANVAS_SETTINGS;
  if (scale < s.zoomFarThreshold) return "far";
  if (scale < s.zoomNearThreshold) return "mid";
  return "near";
}
