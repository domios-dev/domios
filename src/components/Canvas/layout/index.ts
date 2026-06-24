import type { Plan } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import { layoutPlanIntra } from "./intraPlan";
import { packRegions } from "./regionPacking";
import type { NodeModeResolver, WorkspaceLayout } from "./types";

// Every node Document-sized unless a resolver overrides — preserves prior layout.
const DOCUMENT_MODE: NodeModeResolver = () => "document";

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
  NodeModeResolver,
} from "./types";
export { layoutPlanIntra, ownerKeyOf } from "./intraPlan";
export { packRegions } from "./regionPacking";
export { focusAt } from "./focus";
export type { FocusResult } from "./focus";
export { visibleNodeIds, visibleFrameIds, viewportWorldRect } from "./viewport";
export type { CanvasSize } from "./viewport";

// Top-level deterministic layout (§6.1): only active plans, intra-plan cascade
// then region packing. Same data + settings → identical positions.
export function layoutWorkspace(
  plans: Plan[],
  settings: CanvasLayoutSettings = DEFAULT_CANVAS_SETTINGS,
  modeFor: NodeModeResolver = DOCUMENT_MODE,
): WorkspaceLayout {
  const active = plans.filter((p) => p.status === "active");
  const intra = active.map((p) => layoutPlanIntra(p, settings, modeFor));
  return packRegions(intra, settings);
}
