// Fixed river geometry (px within a 1180-wide band) — stations + curves share
// these so the flow lines always dock to station edges. The pan/zoom layer
// scales the whole band; positions stay internally consistent.

export const RIVER_WIDTH = 1180;
export const RIVER_HEIGHT = 190;
export const RIVER_HEIGHT_MINI = 120;

export interface StationGeom {
  left: number;
  width: number;
}

export const STATION_GEOM = {
  frontier: { left: 6, width: 170 },
  milestone: { left: 362, width: 170 },
  verify: { left: 722, width: 170 },
  done: { left: 1032, width: 130 },
} as const satisfies Record<string, StationGeom>;

export type FlowStationKey = keyof typeof STATION_GEOM;

const CENTER_Y = 95;

/** Edge midpoints of each curve segment, for the pulse-dot + carrier anchors. */
export const FLOW_EDGES = [
  { from: "frontier", to: "milestone" },
  { from: "milestone", to: "verify" },
  { from: "verify", to: "done" },
] as const;

function rightEdge(key: FlowStationKey): number {
  return STATION_GEOM[key].left + STATION_GEOM[key].width;
}

export interface FlowCurve {
  from: FlowStationKey;
  to: FlowStationKey;
  d: string;
}

export function flowCurvePaths(): FlowCurve[] {
  return FLOW_EDGES.map(({ from, to }) => {
    const x0 = rightEdge(from);
    const x1 = STATION_GEOM[to].left;
    const dx = (x1 - x0) * 0.45;
    const wobble = to === "milestone" || from === "milestone" ? 7 : 3;
    const d = `M${x0} ${CENTER_Y} C ${x0 + dx} ${CENTER_Y}, ${x1 - dx} ${CENTER_Y - wobble}, ${x1} ${CENTER_Y - wobble}`;
    return { from, to, d };
  });
}

/** The active edge (milestone → verify) — where the pulse + carrier travel. */
export function activeEdgeRange(): { from: number; to: number; y: number } {
  return { from: rightEdge("milestone") + 8, to: STATION_GEOM.verify.left - 7, y: CENTER_Y - 4 };
}

export const CARRIER_LEFT = rightEdge("milestone") + 33;
export const CARRIER_TOP = 88;
