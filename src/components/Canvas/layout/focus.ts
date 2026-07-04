import type { CanvasLayoutSettings } from "../settings";
import { boxCenter, boxContains, dist2 } from "./geometry";
import type { Point } from "./geometry";
import type { PlanFrame, WorkspaceLayout } from "./types";

export interface FocusResult {
  planId: string | null;
  sectionIds: string[];
  taskIds: string[];
  milestoneIds: string[];
}

function pickFrame(frames: PlanFrame[], center: Point): PlanFrame | null {
  if (!frames.length) return null;
  for (const f of frames) if (boxContains(f.box, center)) return f;
  let best = frames[0];
  let bestD = dist2(boxCenter(best.box), center);
  for (const f of frames) {
    const d = dist2(boxCenter(f.box), center);
    if (d < bestD) {
      best = f;
      bestD = d;
    }
  }
  return best;
}

export function focusAt(layout: WorkspaceLayout, center: Point, s: CanvasLayoutSettings): FocusResult {
  const frame = pickFrame(layout.frames, center);
  if (!frame) return { planId: null, sectionIds: [], taskIds: [], milestoneIds: [] };

  const r2 = s.focusZoneRadius * s.focusZoneRadius;
  const sectionIds: string[] = [];
  const taskIds: string[] = [];
  const milestoneIds: string[] = [];

  for (const n of frame.nodes) {
    if (dist2(boxCenter(n.box), center) > r2) continue;
    if (n.kind === "task") taskIds.push(n.id);
    else if (n.kind === "milestone") milestoneIds.push(n.id);
  }
  // Sections are anchors, not nodes; pick the ones whose row sits in the zone.
  for (const [id, p] of frame.anchors) {
    if (dist2(p, center) <= r2) sectionIds.push(id);
  }

  return { planId: frame.planId, sectionIds, taskIds, milestoneIds };
}
