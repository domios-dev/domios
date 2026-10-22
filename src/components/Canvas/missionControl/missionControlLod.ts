import { CANVAS_MAX_SCALE, CANVAS_MIN_SCALE } from "@codeterm/canvas-camera";

export type MissionControlLodLevel = "far" | "mid" | "close";

/** Scale at/below → far LOD (zone silhouettes + counts only). */
export const MC_LOD_FAR_MAX = 0.42;
/** Scale at/below → mid LOD (bays + avatars); above → close (full cards). */
export const MC_LOD_MID_MAX = 0.78;

export function resolveMissionControlLod(scale: number): MissionControlLodLevel {
  const clamped = Math.max(CANVAS_MIN_SCALE, Math.min(CANVAS_MAX_SCALE, scale));
  if (clamped <= MC_LOD_FAR_MAX) return "far";
  if (clamped <= MC_LOD_MID_MAX) return "mid";
  return "close";
}

export function missionControlLodDataAttr(level: MissionControlLodLevel): MissionControlLodLevel {
  return level;
}
