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

export { MissionControlPrototype } from "./MissionControlPrototype";
export { MISSION_CONTROL_FIXTURE, activeLaneCount, dispatchFrontierCount, totalBlockedParked } from "./fixtures";
export type { MissionControlSnapshot, MissionControlVariant, MissionControlLod } from "./types";
export { MISSION_CONTROL_VARIANTS, VARIANT_LABELS } from "./types";

export const MISSION_CONTROL_PROTOTYPE_QUERY = "mission-control";

export function isMissionControlPrototypeRoute(search = typeof window !== "undefined" ? window.location.search : ""): boolean {
  return new URLSearchParams(search).get("prototype") === MISSION_CONTROL_PROTOTYPE_QUERY;
}
