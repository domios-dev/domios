import type { OrchestratorRef, Plan, Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { planNodeHeight } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { buildLaneIndex, layoutCluster, type ClusterResult } from "./clusters";
import { estimateTaskHeight } from "./taskSize";
import { estimateSpecHeight } from "./specSize";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode } from "./types";

export function ownerKeyOf(owner: OrchestratorRef): string {
  return owner.session_id ?? owner.pane_id ?? owner.title ?? "__unowned__";
}

function hasSpec(plan: Plan): boolean {
  const flat = flatSections(plan);
  return flat.length > 0;
}

// Compact-plan + spec + satellite-tasks + bubble-clusters (§2). Local coords with
// frame origin at (0,0); region packing offsets later. Section rows are anchors,
// not nodes — tasks connect by curved edges to the exact row.
export function layoutPlanIntra(plan: Plan, s: CanvasLayoutSettings): IntraPlanLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const anchors = new Map<string, Point>();
  const extraPoints: Point[] = [];

  const flat = flatSections(plan);
  const specPresent = hasSpec(plan);

  const specX = 0;
  const planX = specPresent ? s.specWidth + s.columnGapX : 0;
  const planHeight = planNodeHeight(flat.length, s);

  const planBox: Box = { x: planX, y: 0, width: s.planWidth, height: planHeight };
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  // Per-section-row anchors: RIGHT edge → satellite tasks; LEFT edge → the
  // expanded spec-section card for that same row (spec[i] ↔ plan-row[i]).
  const leftAnchors = new Map<string, Point>();
  flat.forEach((section, i) => {
    const rowMidY = planBox.y + s.planHeaderHeight + i * s.planRowHeight + s.planRowHeight / 2;
    anchors.set(section.id, { x: planBox.x + planBox.width - s.anchorInset, y: rowMidY });
    leftAnchors.set(section.id, { x: planBox.x + s.anchorInset, y: rowMidY });
  });

  // Spec column: one expanded card per section, stacked left of the plan node,
  // each tied by a curved edge to its plan-node row.
  let specY = 0;
  for (const section of flat) {
    const h = estimateSpecHeight(section, s);
    const specId = `${plan.id}::spec::${section.id}`;
    nodes.push({
      id: specId,
      kind: "spec-section",
      planId: plan.id,
      box: { x: specX, y: specY, width: s.specWidth, height: h },
      iteration: null,
      manual: false,
    });
    const left = leftAnchors.get(section.id)!;
    edges.push({ id: `${specId}~row`, kind: "spec-row", from: specId, toPoint: left });
    specY += h + s.specSectionGapY;
  }

  const lanes = buildLaneIndex(plan);
  const taskX = planX + s.planWidth + s.columnGapX;

  // Place a task's milestone bubble cluster to the right of its card.
  const pushCluster = (task: Task, box: Box, cluster: ClusterResult) => {
    if (!cluster.nodes.length) return;
    const clusterX = box.x + box.width + s.clusterGapX;
    const clusterY = box.y + Math.max(0, (box.height - cluster.height) / 2);
    const pad = 8;
    const labelH = s.clusterLabelHeight;
    nodes.push({
      id: `${task.id}::cluster`,
      kind: "cluster",
      planId: plan.id,
      box: { x: clusterX - pad, y: clusterY - pad - labelH, width: cluster.width + pad * 2, height: cluster.height + pad * 2 + labelH },
      iteration: null,
      manual: false,
    });
    for (const cn of cluster.nodes) {
      nodes.push({ ...cn, box: { ...cn.box, x: cn.box.x + clusterX, y: cn.box.y + clusterY } });
    }
    edges.push(...cluster.edges);
    const clusterAnchor: Point = { x: clusterX, y: clusterY + cluster.height / 2 };
    extraPoints.push(clusterAnchor, { x: clusterX + cluster.width, y: clusterAnchor.y });
    edges.push({ id: `${task.id}~cluster`, kind: "task-cluster", from: task.id, toPoint: clusterAnchor });
  };

  // Subtasks (parent_task_id): children render nested under their parent, not as
  // top-level satellites. Only top-level tasks are bin-packed near their rows.
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

  const placements = topLevel.map((task, i) => {
    const primarySid = task.section_ids.find((sid) => anchors.has(sid));
    const desired = primarySid != null ? anchors.get(primarySid)!.y : null;
    const parentHeight = estimateTaskHeight(task, s);
    const children = childrenByParent.get(task.id) ?? [];
    // A parent groups its children; its own milestones fold into the children.
    const parentCluster = children.length ? null : layoutCluster(task, plan.id, lanes, s);
    const childItems = children.map((c) => ({ task: c, height: estimateTaskHeight(c, s), cluster: layoutCluster(c, plan.id, lanes, s) }));
    let childrenH = 0;
    if (childItems.length) {
      childrenH = s.subtaskHeaderGap + s.subtaskGroupPad * 2 - s.subtaskGapY;
      for (const ci of childItems) childrenH += Math.max(ci.height, ci.cluster.height) + s.subtaskGapY;
    }
    const rowSpan = childItems.length ? parentHeight + childrenH : Math.max(parentHeight, parentCluster?.height ?? 0);
    return { task, i, desired, parentHeight, parentCluster, childItems, rowSpan };
  });
  placements.sort((a, b) => {
    if (a.desired == null && b.desired == null) return a.i - b.i;
    if (a.desired == null) return 1;
    if (b.desired == null) return -1;
    return a.desired - b.desired || a.i - b.i;
  });

  let cursor = 0;
  for (const { task, desired, parentHeight, parentCluster, childItems, rowSpan } of placements) {
    const wanted = desired != null ? desired - parentHeight / 2 : cursor;
    const y = Math.max(cursor, wanted);

    const box: Box = { x: taskX, y, width: s.taskWidth, height: parentHeight };
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });

    for (const sid of task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
    }
    if (parentCluster) pushCluster(task, box, parentCluster);

    if (childItems.length) {
      const childX = taskX + s.subtaskIndentX;
      const childW = s.taskWidth - s.subtaskIndentX;
      const groupTop = y + parentHeight + s.subtaskHeaderGap;
      let childY = groupTop + s.subtaskGroupPad;
      const placed: { item: (typeof childItems)[number]; box: Box }[] = [];
      for (const ci of childItems) {
        const cbox: Box = { x: childX, y: childY, width: childW, height: ci.height };
        placed.push({ item: ci, box: cbox });
        childY += Math.max(ci.height, ci.cluster.height) + s.subtaskGapY;
      }
      const groupBottom = childY - s.subtaskGapY + s.subtaskGroupPad;
      // Soft container behind the children (pushed first → sits under the cards).
      nodes.push({
        id: `${task.id}::subgroup`,
        kind: "subtask-group",
        planId: plan.id,
        box: { x: childX - s.subtaskGroupPad, y: groupTop, width: childW + s.subtaskGroupPad * 2, height: groupBottom - groupTop },
        iteration: null,
        manual: false,
      });
      for (const { item, box: cbox } of placed) {
        nodes.push({ id: item.task.id, kind: "task", planId: plan.id, box: cbox, iteration: null, manual: false });
        edges.push({ id: `${task.id}>${item.task.id}`, kind: "subtask", from: task.id, to: item.task.id });
        pushCluster(item.task, cbox, item.cluster);
      }
    }

    cursor = y + rowSpan + s.taskGapY;
  }

  // Free-form elements (open registry kinds) render only when explicitly placed;
  // unpositioned auto-layout is deferred. Manual coords keep determinism.
  for (const el of plan.elements) {
    if (el.canvas.x == null || el.canvas.y == null) continue;
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: s.taskWidth, height: s.taskMinHeight };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
  }

  // Vertically center the two columns (spec-section column vs plan+tasks block)
  // against each other so the content band is balanced, not top-weighted.
  const isBlock = (k: string) => k === "plan" || k === "task" || k === "milestone" || k === "cluster" || k === "subtask-group";
  const specBoxes = nodes.filter((n) => n.kind === "spec-section").map((n) => n.box);
  const blockBoxes = nodes.filter((n) => isBlock(n.kind)).map((n) => n.box);
  if (specBoxes.length && blockBoxes.length) {
    const specTop = Math.min(...specBoxes.map((b) => b.y));
    const specH = Math.max(...specBoxes.map((b) => b.y + b.height)) - specTop;
    const blockTop = Math.min(...blockBoxes.map((b) => b.y));
    const blockH = Math.max(...blockBoxes.map((b) => b.y + b.height)) - blockTop;
    const contentH = Math.max(specH, blockH);
    const specOff = (contentH - specH) / 2 - specTop;
    const blockOff = (contentH - blockH) / 2 - blockTop;
    for (let i = 0; i < nodes.length; i++) {
      const off = nodes[i].kind === "spec-section" ? specOff : isBlock(nodes[i].kind) ? blockOff : 0;
      if (off) nodes[i] = { ...nodes[i], box: { ...nodes[i].box, y: nodes[i].box.y + off } };
    }
    for (const [k, p] of anchors) anchors.set(k, { x: p.x, y: p.y + blockOff });
    // Every edge POINT is a plan-row/cluster anchor (block-side); the spec end of
    // a spec-row edge is a node and moves with specOff automatically.
    for (const e of edges) {
      if (e.fromPoint) e.fromPoint = { x: e.fromPoint.x, y: e.fromPoint.y + blockOff };
      if (e.toPoint) e.toPoint = { x: e.toPoint.x, y: e.toPoint.y + blockOff };
    }
  }

  const autoBoxes = nodes.map((n) => n.box);
  const content = unionBox([
    ...autoBoxes,
    ...[...anchors.values(), ...extraPoints].map((p) => ({ x: p.x, y: p.y, width: 0, height: 0 })),
  ]);
  const width = content.width + s.framePadding * 2;
  const height = content.height + s.framePadding * 2;

  // Shift everything by framePadding so nothing sits on the frame edge.
  const dx = s.framePadding - content.x;
  const dy = s.framePadding - content.y;
  for (let i = 0; i < nodes.length; i++) {
    nodes[i] = { ...nodes[i], box: { ...nodes[i].box, x: nodes[i].box.x + dx, y: nodes[i].box.y + dy } };
  }
  for (const [k, p] of anchors) anchors.set(k, { x: p.x + dx, y: p.y + dy });
  for (const e of edges) {
    if (e.fromPoint) e.fromPoint = { x: e.fromPoint.x + dx, y: e.fromPoint.y + dy };
    if (e.toPoint) e.toPoint = { x: e.toPoint.x + dx, y: e.toPoint.y + dy };
  }

  return {
    planId: plan.id,
    title: plan.title,
    owner: plan.owner,
    ownerKey: ownerKeyOf(plan.owner),
    updatedAt: plan.updated_at,
    width,
    height,
    nodes,
    edges,
    anchors,
  };
}
