import type { Plan, Task, TaskStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { flatSections } from "../sectionRows";
import { zoneLabelBox } from "./sectionZones";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";
import { UNSECTIONED } from "./taskCloud";
import type { TaskCloudGroup } from "./taskCloud";

function priorityRank(p: Task["priority"]): number {
  switch (p) {
    case "p0": return 0;
    case "p1": return 1;
    case "p3": return 3;
    default: return 2;
  }
}

function isSunk(status: TaskStatus): boolean {
  return status === "done" || status === "cancelled";
}

function sortTasksForGrid(tasks: Task[]): Task[] {
  const statusRank = (s: TaskStatus) => (s === "in_progress" ? 0 : isSunk(s) ? 2 : 1);
  return [...tasks].sort(
    (a, b) =>
      statusRank(a.status) - statusRank(b.status) ||
      priorityRank(a.priority) - priorityRank(b.priority) ||
      (b.status_changed_at ?? 0) - (a.status_changed_at ?? 0) ||
      (a.created_at ?? 0) - (b.created_at ?? 0) ||
      a.id.localeCompare(b.id),
  );
}

function orderGroups(keys: string[], plan: Plan): string[] {
  const present = new Set(keys);
  const flatOrder = flatSections(plan).map((sec) => sec.id).filter((id) => present.has(id));
  const seen = new Set(flatOrder);
  const extras = keys.filter((k) => k !== UNSECTIONED && !seen.has(k)).sort();
  const tail = present.has(UNSECTIONED) ? [UNSECTIONED] : [];
  return [...flatOrder, ...extras, ...tail];
}

export interface ZoneGridLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
  groups: TaskCloudGroup[];
  zoneBoxes: Map<string, Box>;
}

export function layoutZoneGrid(
  plan: Plan,
  s: CanvasLayoutSettings,
  anchors: Map<string, Point>,
  sectionTitles?: ReadonlyMap<string, string>,
): ZoneGridLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];
  const zoneBoxes = new Map<string, Box>();

  const planTitles = new Map(flatSections(plan).map((sec) => [sec.id, sec.title]));
  const titleOf = (sid: string): string | null => {
    const t = (sectionTitles?.get(sid) ?? planTitles.get(sid))?.trim();
    return t || null;
  };

  const taskIds = new Set(plan.tasks.map((t) => t.id));
  const taskById = new Map(plan.tasks.map((t) => [t.id, t]));

  const resolveSection = (t: Task): string => {
    if (t.section_ids[0]) return t.section_ids[0];
    const pid = t.parent_task_id;
    if (pid && taskIds.has(pid)) {
      const p = taskById.get(pid);
      if (p?.section_ids[0]) return p.section_ids[0];
    }
    return UNSECTIONED;
  };

  const byGroup = new Map<string, Task[]>();
  for (const t of plan.tasks) {
    const g = resolveSection(t);
    const arr = byGroup.get(g);
    if (arr) arr.push(t);
    else byGroup.set(g, [t]);
  }

  function gridOrder(tasks: Task[]): Task[] {
    const locals = new Map(tasks.map((t) => [t.id, t]));
    const roots = tasks.filter((t) => !t.parent_task_id || !locals.has(t.parent_task_id));
    const out: Task[] = [];
    for (const p of sortTasksForGrid(roots)) {
      out.push(p);
      const kids = tasks
        .filter((tt) => tt.parent_task_id === p.id)
        .sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id));
      out.push(...kids);
    }
    for (const t of sortTasksForGrid(tasks.filter((tt) => !out.includes(tt)))) out.push(t);
    return out;
  }

  const groupOrder = orderGroups([...byGroup.keys()], plan);
  const cellW = s.zoneTaskW;
  const cellH = s.zoneTaskH;
  const gap = s.zoneTaskGap;
  const cols = Math.max(1, s.zoneCols);
  const pad = s.zonePad;

  const zoneWidths = new Map<string, number>();
  const zoneHeights = new Map<string, number>();
  const zoneLabelHeights = new Map<string, number>();

  for (const sectionId of groupOrder) {
    const tasks = gridOrder(byGroup.get(sectionId) ?? []);
    if (!tasks.length) continue;
    const rows = Math.ceil(tasks.length / cols);
    const title = sectionId === UNSECTIONED ? null : titleOf(sectionId);
    const labelH = title ? zoneLabelBox(title).height + s.zoneLabelGap : 0;
    const gridW = cols * cellW + (cols - 1) * gap;
    const gridH = rows * cellH + (rows - 1) * gap;
    zoneWidths.set(sectionId, gridW + pad * 2);
    zoneHeights.set(sectionId, labelH + gridH + pad * 2);
    zoneLabelHeights.set(sectionId, labelH);
  }

  let stackX = 0;
  const zoneOrigins = new Map<string, Point>();

  for (const sectionId of groupOrder) {
    const zw = zoneWidths.get(sectionId);
    const zh = zoneHeights.get(sectionId);
    if (!zw || !zh) continue;
    zoneOrigins.set(sectionId, { x: stackX, y: 0 });
    zoneBoxes.set(sectionId, { x: stackX, y: 0, width: zw, height: zh });
    stackX += zw + s.zoneGap;
  }

  const fieldOffsetY = 0;

  const boxesBySection = new Map<string, Box[]>();
  const labelAnchors = new Map<string, Point>();

  for (const sectionId of groupOrder) {
    const tasks = gridOrder(byGroup.get(sectionId) ?? []);
    const origin = zoneOrigins.get(sectionId);
    const zbox = zoneBoxes.get(sectionId);
    if (!origin || !zbox || !tasks.length) continue;

    const zx = origin.x;
    const zy = origin.y + fieldOffsetY;
    zoneBoxes.set(sectionId, { x: zx, y: zy, width: zbox.width, height: zbox.height });

    const labelH = zoneLabelHeights.get(sectionId) ?? 0;
    const title = sectionId === UNSECTIONED ? null : titleOf(sectionId);
    if (title) {
      const lb = zoneLabelBox(title);
      labelAnchors.set(sectionId, { x: zx + zbox.width / 2, y: zy + pad + lb.height / 2 });
    }

    const gridTop = zy + labelH + pad;
    const gridLeft = zx + pad;
    const boxes: Box[] = [];

    tasks.forEach((task, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      const x = gridLeft + col * (cellW + gap);
      const y = gridTop + row * (cellH + gap);
      const box: Box = { x, y, width: cellW, height: cellH };
      nodes.push({ id: task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });
      boxes.push(box);
      for (const linkSid of task.section_ids) {
        const anchor = anchors.get(linkSid);
        if (anchor) edges.push({ id: `${linkSid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
      }
    });
    boxesBySection.set(sectionId, boxes);
  }

  const groups: TaskCloudGroup[] = groupOrder
    .filter((sid) => (boxesBySection.get(sid)?.length ?? 0) > 0)
    .map((sid) => ({
      sectionId: sid,
      boxes: boxesBySection.get(sid)!,
      labelAnchor: labelAnchors.get(sid),
    }));

  return { nodes, edges, extraPoints, groups, zoneBoxes };
}
