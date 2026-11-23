export interface HoloBudget { points: number; edges: number }

function distributed<T>(values: readonly T[], limit: number): T[] {
  const count = Math.min(values.length, limit);
  return Array.from({ length: count }, (_, slot) => values[Math.floor(slot * values.length / count)]);
}

/** Keep the focal point, then sample the original topology without introducing dangling edges. */
export function sampleHoloTopology(
  count: number,
  edges: ReadonlyArray<readonly [number, number]>,
  focal: number,
  budget: HoloBudget,
) {
  const points = [focal, ...distributed(Array.from({ length: count }, (_, index) => index).filter((index) => index !== focal), budget.points - 1)];
  const kept = new Set(points);
  const eligible = edges.flatMap(([a, b], index) => kept.has(a) && kept.has(b) ? [index] : []);
  return { points, edges: distributed(eligible, budget.edges) };
}
