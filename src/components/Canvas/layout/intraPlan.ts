import type { OrchestratorRef, Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { planNodeHeight } from "../settings";
import { flatSections } from "../sectionRows";
import type { Box, Point } from "./geometry";
import { unionBox } from "./geometry";
import { buildLaneIndex, layoutCluster } from "./clusters";
import { estimateTaskHeight } from "./taskSize";
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

  const specNodeId = `${plan.id}::spec`;
  if (specPresent) {
    nodes.push({
      id: specNodeId,
      kind: "spec",
      planId: plan.id,
      box: { x: specX, y: 0, width: s.specWidth, height: s.specHeight },
      iteration: null,
      manual: false,
    });
  }

  const planBox: Box = { x: planX, y: 0, width: s.planWidth, height: planHeight };
  nodes.push({ id: plan.id, kind: "plan", planId: plan.id, box: planBox, iteration: null, manual: false });

  if (specPresent) {
    edges.push({ id: `${plan.id}~spec`, kind: "plan-spec", from: plan.id, to: specNodeId });
  }

  flat.forEach((section, i) => {
    anchors.set(section.id, {
      x: planBox.x + planBox.width - s.anchorInset,
      y: planBox.y + s.planHeaderHeight + i * s.planRowHeight + s.planRowHeight / 2,
    });
  });

  const lanes = buildLaneIndex(plan);
  const taskX = planX + s.planWidth + s.columnGapX;

  // Bin-pack tasks near the Y of the section row(s) they cover so edges stay
  // short and the board has no dead vertical space. Stable + deterministic:
  // sort by desired Y (uncovered tasks keep input order, appended), then place
  // greedily top-down so nothing overlaps.
  const placements = plan.tasks.map((task, i) => {
    // Primary covered row = first section_id with an anchor; its Y anchors the
    // task's vertical center so the section→task edge runs level.
    const primarySid = task.section_ids.find((sid) => anchors.has(sid));
    const desired = primarySid != null ? anchors.get(primarySid)!.y : null;
    return { task, i, desired, height: estimateTaskHeight(task, s), cluster: layoutCluster(task, plan.id, lanes, s) };
  });
  placements.sort((a, b) => {
    if (a.desired == null && b.desired == null) return a.i - b.i;
    if (a.desired == null) return 1;
    if (b.desired == null) return -1;
    return a.desired - b.desired || a.i - b.i;
  });

  let cursor = 0;
  for (const { task, desired, height, cluster } of placements) {
    const rowHeight = Math.max(height, cluster.height);
    const wanted = desired != null ? desired - height / 2 : cursor;
    const y = Math.max(cursor, wanted);

    const box: Box = { x: taskX, y, width: s.taskWidth, height };
    nodes.push({ id: task.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });

    for (const sid of task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${task.id}`, kind: "section-task", fromPoint: anchor, to: task.id });
    }

    if (cluster.nodes.length) {
      const clusterX = box.x + box.width + s.clusterGapX;
      const clusterY = box.y + Math.max(0, (height - cluster.height) / 2);
      const pad = 8;
      // Faint container behind the cluster (rendered first → sits under bubbles).
      nodes.push({
        id: `${task.id}::cluster`,
        kind: "cluster",
        planId: plan.id,
        box: { x: clusterX - pad, y: clusterY - pad, width: cluster.width + pad * 2, height: cluster.height + pad * 2 },
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
    }

    cursor = y + rowHeight + s.taskGapY;
  }

  // Free-form elements (open registry kinds) render only when explicitly placed;
  // unpositioned auto-layout is deferred. Manual coords keep determinism.
  for (const el of plan.elements) {
    if (el.canvas.x == null || el.canvas.y == null) continue;
    const box: Box = { x: el.canvas.x, y: el.canvas.y, width: s.taskWidth, height: s.taskMinHeight };
    nodes.push({ id: el.id, kind: el.kind, planId: plan.id, box, iteration: null, manual: true });
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
