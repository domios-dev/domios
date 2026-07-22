export {
  buildMissionControl,
  MISSION_ZONE_DEFS,
  type MissionControlModel,
  type MissionZone,
  type MissionZoneDef,
  type MissionZoneKey,
  type TaskStation,
  type WorkerPresence,
} from "./missionControl";

export { modelToSnapshot, missionControlHealth, extractStreamEvents } from "./modelAdapter";
export { buildFlowRiver, type FlowRiverModel, type MilestoneStation } from "./flowModel";
export { PlanRiver } from "./PlanRiver";
export { resolveMissionControlLod, MC_LOD_FAR_MAX, MC_LOD_MID_MAX, type MissionControlLodLevel } from "./missionControlLod";
export { MissionControlLiveView } from "./MissionControlLiveView";
export { MissionControlPlanSurface } from "./MissionControlPlanSurface";
export { MissionControlPrototype } from "./MissionControlPrototype";
export { useReducedMotion } from "./useReducedMotion";
export { missionControlInputRevision } from "./missionControlInputRevision";
export { MissionControlStage } from "./MissionControlStage";
export { MISSION_CONTROL_FIXTURE, activeLaneCount, dispatchFrontierCount, totalBlockedPaused } from "./fixtures";
export type { MissionControlSnapshot, MissionControlVariant, MissionControlLod } from "./types";
export { MISSION_CONTROL_VARIANTS, VARIANT_LABELS } from "./types";

export const MISSION_CONTROL_PROTOTYPE_QUERY = "mission-control";

export function isMissionControlPrototypeRoute(search = typeof window !== "undefined" ? window.location.search : ""): boolean {
  return new URLSearchParams(search).get("prototype") === MISSION_CONTROL_PROTOTYPE_QUERY;
}
