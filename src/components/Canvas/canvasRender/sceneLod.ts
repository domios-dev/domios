import type { Box } from "../layout";
import { boxCenter, unionBox } from "../layout/geometry";
import type { PresenceAgent } from "../presence/identity";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import type { Task, TaskStatus } from "../types";

import { CANVAS_MIN_SCALE } from "../useCanvasViewport";

export { CANVAS_MIN_SCALE };

export const OVERVIEW_LOD_SCALE = DEFAULT_CANVAS_SETTINGS.zoomFarThreshold;
const CHIP_SCREEN_W = 120;
const CHIP_SCREEN_H = 58;
const DONE_STACK_SCREEN_W = 72;
const DONE_STACK_SCREEN_H = 36;

export type LodMode = "overview" | "readable";

export function lodMode(scale: number): LodMode {
  return scale <= OVERVIEW_LOD_SCALE ? "overview" : "readable";
}

export interface StatusCounts {
  pending: number;
  in_progress: number;
  in_review: number;
  blocked: number;
  paused: number;
  done: number;
  cancelled: number;
}

export function emptyCounts(): StatusCounts {
  return { pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 };
}

export function bumpCount(counts: StatusCounts, status: TaskStatus): void {
  counts[status] += 1;
}

export function isTerminalStatus(status: TaskStatus): boolean {
  return status === "done" || status === "cancelled";
}

export interface SectionTaskGroup {
  planId: string;
  sectionId: string;
  title: string;
  taskNodeIds: string[];
  nodeBoxById: Map<string, Box>;
  counts: StatusCounts;
  presence: PresenceAgent[];
  anchor: Box;
}

export interface LodMarker {
  id: string;
  kind: "section-cluster" | "done-stack";
  planId: string;
  sectionId: string;
  title: string;
  box: Box;
  counts: StatusCounts;
  presence: PresenceAgent[];
}

export interface SceneLodNode {
  id: string;
  kind: string;
  planId: string;
  box: Box;
}

export function chipWorldBox(anchor: Box, scale: number, screenW: number, screenH: number): Box {
  const w = screenW / scale;
  const h = screenH / scale;
  const c = boxCenter(anchor);
  return { x: c.x - w / 2, y: c.y - h / 2, width: w, height: h };
}

export function buildSectionGroups(
  nodes: readonly SceneLodNode[],
  taskById: Map<string, Task>,
  sectionTitleOf: (planId: string, sectionId: string) => string,
  presenceBySection: ReadonlyMap<string, PresenceAgent[]>,
): SectionTaskGroup[] {
  const byKey = new Map<string, { planId: string; sectionId: string; boxes: Box[]; ids: string[]; nodeBoxById: Map<string, Box>; counts: StatusCounts }>();
  for (const node of nodes) {
    if (node.kind !== "task") continue;
    const task = taskById.get(node.id);
    if (!task) continue;
    const sectionId = task.section_ids[0] ?? "__unsectioned__";
    const key = `${node.planId}:${sectionId}`;
    let g = byKey.get(key);
    if (!g) {
      g = { planId: node.planId, sectionId, boxes: [], ids: [], nodeBoxById: new Map(), counts: emptyCounts() };
      byKey.set(key, g);
    }
    g.boxes.push(node.box);
    g.ids.push(node.id);
    g.nodeBoxById.set(node.id, node.box);
    bumpCount(g.counts, task.status);
  }
  const out: SectionTaskGroup[] = [];
  for (const g of byKey.values()) {
    const anchor = unionBox(g.boxes);
    if (anchor.width <= 0 || anchor.height <= 0) continue;
    out.push({
      planId: g.planId,
      sectionId: g.sectionId,
      title: sectionTitleOf(g.planId, g.sectionId),
      taskNodeIds: g.ids,
      nodeBoxById: g.nodeBoxById,
      counts: g.counts,
      presence: presenceBySection.get(`${g.planId}:${g.sectionId}`) ?? [],
      anchor,
    });
  }
  return out;
}

export interface ResolvedSceneLod {
  mode: LodMode;
  hiddenNodeIds: Set<string>;
  markers: LodMarker[];
}

export function resolveSceneLod(
  scale: number,
  groups: readonly SectionTaskGroup[],
  taskById: Map<string, Task>,
): ResolvedSceneLod {
  const mode = lodMode(scale);
  const hiddenNodeIds = new Set<string>();
  const markers: LodMarker[] = [];

  if (mode === "overview") {
    for (const g of groups) {
      for (const id of g.taskNodeIds) hiddenNodeIds.add(id);
      if (g.taskNodeIds.length === 0) continue;
      markers.push({
        id: `cluster:${g.planId}:${g.sectionId}`,
        kind: "section-cluster",
        planId: g.planId,
        sectionId: g.sectionId,
        title: g.title,
        box: chipWorldBox(g.anchor, scale, CHIP_SCREEN_W, CHIP_SCREEN_H),
        counts: g.counts,
        presence: g.presence,
      });
    }
    return { mode, hiddenNodeIds, markers };
  }

  for (const g of groups) {
    const doneBoxes: Box[] = [];
    const doneIds: string[] = [];
    for (const id of g.taskNodeIds) {
      const task = taskById.get(id);
      if (!task || !isTerminalStatus(task.status)) continue;
      hiddenNodeIds.add(id);
      doneIds.push(id);
      const box = g.nodeBoxById.get(id);
      if (box) doneBoxes.push(box);
    }
    if (!doneBoxes.length) continue;
    const anchor = unionBox(doneBoxes);
    const doneOnly = emptyCounts();
    for (const id of doneIds) {
      const task = taskById.get(id);
      if (task) bumpCount(doneOnly, task.status);
    }
    markers.push({
      id: `done:${g.planId}:${g.sectionId}`,
      kind: "done-stack",
      planId: g.planId,
      sectionId: g.sectionId,
      title: g.title,
      box: chipWorldBox(anchor, scale, DONE_STACK_SCREEN_W, DONE_STACK_SCREEN_H),
      counts: doneOnly,
      presence: [],
    });
  }
  return { mode, hiddenNodeIds, markers };
}

export function markerToSyntheticNode(m: LodMarker): SceneLodNode {
  return { id: m.id, kind: m.kind, planId: m.planId, box: m.box };
}
