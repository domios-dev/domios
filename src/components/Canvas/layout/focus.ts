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

// Focus detection (§6.1): the frame under/nearest the viewport center is the
// focused plan (its owner is the talk-to-area target); nodes inside the focus
// zone are the in-view documents.
export function focusAt(layout: WorkspaceLayout, center: Point, s: CanvasLayoutSettings): FocusResult {
  const frame = pickFrame(layout.frames, center);
  if (!frame) return { planId: null, sectionIds: [], taskIds: [], milestoneIds: [] };

  const r2 = s.focusZoneRadius * s.focusZoneRadius;
  const sectionIds: string[] = [];
  const taskIds: string[] = [];
  const milestoneIds: string[] = [];

  for (const n of frame.nodes) {
    if (dist2(boxCenter(n.box), center) > r2) continue;
    if (n.kind === "section") sectionIds.push(n.id);
    else if (n.kind === "task") taskIds.push(n.id);
    else milestoneIds.push(n.id);
  }

  return { planId: frame.planId, sectionIds, taskIds, milestoneIds };
}
