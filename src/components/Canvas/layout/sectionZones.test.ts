// Run: npx tsx src/components/Canvas/layout/sectionZones.test.ts

import type { Box, Point } from "./geometry";
import { convexHull, roundPolygon, computeZonePolygon, zoneCentroid, smoothClosedPath, zoneColor, zoneLabelBox } from "./sectionZones";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

// Point-in-convex-polygon (ray cast).
function inside(poly: Point[], p: Point): boolean {
  let c = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i], b = poly[j];
    if ((a.y > p.y) !== (b.y > p.y) && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) c = !c;
  }
  return c;
}
const corners = (b: Box): Point[] => [
  { x: b.x, y: b.y }, { x: b.x + b.width, y: b.y }, { x: b.x, y: b.y + b.height }, { x: b.x + b.width, y: b.y + b.height },
];

// ── convex hull basics ──────────────────────────────────────────────
{
  const pts = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }, { x: 5, y: 5 }];
  const h = convexHull(pts);
  ok("hull of a square + interior point = 4 vertices", h.length === 4);
}

// ── a lone section (no neighbours) → the whole-cloud hull, enclosing its tasks ──
{
  const boxes: Box[] = [
    { x: 0, y: 0, width: 120, height: 90 },
    { x: 200, y: 40, width: 120, height: 90 },
    { x: 90, y: 220, width: 120, height: 90 },
  ];
  const zone = computeZonePolygon(boxes, zoneCentroid(boxes), []);
  ok("lone zone encloses every member box corner", boxes.flatMap(corners).every((p) => inside(zone.hull, p)));
  const minY = Math.min(...zone.hull.map((p) => p.y));
  ok("labelAnchor sits at the hull's topmost edge", Math.abs(zone.labelAnchor.y - minY) < 1e-6);
}

// ── PUZZLE: the cloud hull is partitioned into Voronoi cells — they tessellate the
// SAME cloud, share the bisector border, and never overlap. ──
{
  const left: Box[] = [{ x: 0, y: 0, width: 120, height: 90 }, { x: 0, y: 140, width: 120, height: 90 }];
  const right: Box[] = [{ x: 400, y: 0, width: 120, height: 90 }, { x: 400, y: 140, width: 120, height: 90 }];
  const sl = zoneCentroid(left);
  const sr = zoneCentroid(right);
  const zl = computeZonePolygon(left, sl, [sr]);
  const zr = computeZonePolygon(right, sr, [sl]);
  const midX = (sl.x + sr.x) / 2;
  // Both cells are bounded by the bisector at midX → they meet there and never overlap
  // (the only shared region is that line, measure zero).
  ok("left cell stays on its side of the bisector", zl.hull.every((p) => p.x <= midX + 1e-6));
  ok("right cell stays on its side of the bisector", zr.hull.every((p) => p.x >= midX - 1e-6));
  // Each cell owns its own site and excludes the other's (sites sit clear of the border).
  ok("each cell contains its own site, not the neighbour's", inside(zl.hull, sl) && inside(zr.hull, sr) && !inside(zl.hull, sr) && !inside(zr.hull, sl));
  ok("each cell encloses its own tasks", left.flatMap(corners).every((p) => inside(zl.hull, p)));
}

// ── smoothing: Chaikin rounds, path is a closed bézier ──────────────
{
  const sq = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }];
  ok("roundPolygon adds points (corner-cutting)", roundPolygon(sq, 3).length > sq.length);
  const d = smoothClosedPath(sq);
  ok("smoothed path starts with M, closes with Z, uses béziers", d.startsWith("M ") && d.trimEnd().endsWith("Z") && d.includes(" C "));
}

// ── zone label box: full title (no ellipsis under the cap), wraps to ≤2 lines ──
{
  const short = zoneLabelBox("Worlds & content-LOD");
  ok("short title is one full line, no ellipsis", short.lines.length === 1 && short.lines[0] === "Worlds & content-LOD" && !short.lines[0].includes("…"));
  const long = zoneLabelBox("Worlds & content-LOD plus a very long deliberately overflowing section title here");
  ok("long title wraps to at most 2 lines", long.lines.length <= 2 && long.lines.length === 2);
  ok("long title reserves a taller box than a short one", long.height > short.height);
  ok("long title reserves a wider box than a short one", long.width >= short.width);
}

// ── zone color: deterministic + distinct per section ────────────────
{
  ok("zoneColor is deterministic", zoneColor("sec_core") === zoneColor("sec_core"));
  ok("distinct sections get distinct hues", zoneColor("sec_core") !== zoneColor("sec_perf"));
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
