import type { Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { layoutPlanIntra } from "./intraPlan";
import { attachPlanLineage } from "./lineage";
import { packRegions } from "./regionPacking";
import type { WorkspaceLayout } from "./types";

export type { Box, Point } from "./geometry";
export {
  boxesOverlap,
  boxContains,
  boxCenter,
  unionBox,
  translateBox,
} from "./geometry";
export type {
  CanvasNodeKind,
  CanvasEdgeKind,
  LaidOutNode,
  LaidOutEdge,
  PlanFrame,
  OwnerGroup,
  WorkspaceLayout,
  IntraPlanLayout,
} from "./types";
export { layoutPlanIntra, ownerKeyOf } from "./intraPlan";
export { packRegions } from "./regionPacking";
export { focusAt } from "./focus";
export type { FocusResult } from "./focus";
export { visibleNodeIds, visibleFrameIds, viewportWorldRect } from "./viewport";
export type { CanvasSize } from "./viewport";
export { layoutInputRevision, plansForLayout } from "./layoutInputRevision";
export {
  attachPlanLineage,
  buildLineageCatalog,
  collectPlanLineageEdges,
  ghostPlanNodeId,
} from "./lineage";
export type { PlanLineageEdge, PlanLineageTarget } from "./lineage";

// Top-level deterministic layout (§6.1): only active plans, intra-plan cascade
// then region packing. Same data + settings → identical positions.
export function layoutWorkspace(
  plans: Plan[],
  settings: CanvasLayoutSettings = DEFAULT_CANVAS_SETTINGS,
): WorkspaceLayout {
  const active = plans.filter((p) => p.status === "active");
  const intra = active.map((p) => layoutPlanIntra(p, settings));
  const packed = packRegions(intra, settings);
  return attachPlanLineage(packed, active, settings);
}
