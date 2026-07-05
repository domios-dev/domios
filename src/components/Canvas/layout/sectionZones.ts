import type { Box, Point } from "./geometry";


export const ZONE_PADDING = 24; // outward inflation from the hull (world px)
export const ZONE_GAP = 16; // small breathing gap between adjacent section zones (world px)

export const ZONE_LABEL_FONT = 18;
const LABEL_CHAR_W = ZONE_LABEL_FONT * 0.56;
const LABEL_LINE_H = ZONE_LABEL_FONT * 1.28;
const LABEL_PAD_X = 24;
const LABEL_PAD_Y = 16;
const LABEL_MIN_W = 90;
const LABEL_MAX_W = 320;

export interface ZoneLabelBox {
  width: number;
  height: number;
  lines: string[];
}

export function zoneLabelBox(title: string): ZoneLabelBox {
  const t = title.trim();
  const oneLineW = t.length * LABEL_CHAR_W + LABEL_PAD_X;
  if (oneLineW <= LABEL_MAX_W) {
    return { width: Math.max(LABEL_MIN_W, Math.round(oneLineW)), height: Math.round(LABEL_LINE_H + LABEL_PAD_Y), lines: [t] };
  }
  const maxChars = Math.max(6, Math.floor((LABEL_MAX_W - LABEL_PAD_X) / LABEL_CHAR_W));
  let l0 = "";
  let l1 = "";
  for (const w of t.split(/\s+/)) {
    if (!l1 && (l0 ? l0.length + 1 + w.length : w.length) <= maxChars) l0 = l0 ? `${l0} ${w}` : w;
    else l1 = l1 ? `${l1} ${w}` : w;
  }
  if (!l0) { l0 = t.slice(0, maxChars); l1 = t.slice(maxChars); }
  if (l1.length > maxChars) l1 = `${l1.slice(0, maxChars - 1).trimEnd()}…`;
  const lines = l1 ? [l0, l1] : [l0];
  return { width: LABEL_MAX_W, height: Math.round(lines.length * LABEL_LINE_H + LABEL_PAD_Y), lines };
}

const CIRCLE_SAMPLES = 16;

function circleSamples(boxes: Box[], pad: number): Point[] {
  const pts: Point[] = [];
  for (const b of boxes) {
    const cx = b.x + b.width / 2;
    const cy = b.y + b.height / 2;
    const r = 0.5 * Math.hypot(b.width, b.height) + pad;
    for (let i = 0; i < CIRCLE_SAMPLES; i++) {
      const a = (i / CIRCLE_SAMPLES) * Math.PI * 2;
      pts.push({ x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r });
    }
  }
  return pts;
}

// Andrew's monotone-chain convex hull. Returns CCW hull vertices (no duplicates).
export function convexHull(points: Point[]): Point[] {
  const pts = [...points].sort((a, b) => a.x - b.x || a.y - b.y);
  if (pts.length <= 2) return pts;
  const cross = (o: Point, a: Point, b: Point) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lower: Point[] = [];
  for (const p of pts) {
    while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], p) <= 0) lower.pop();
    lower.push(p);
  }
  const upper: Point[] = [];
  for (let i = pts.length - 1; i >= 0; i--) {
    const p = pts[i];
    while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], p) <= 0) upper.pop();
    upper.push(p);
  }
  lower.pop();
  upper.pop();
  return lower.concat(upper);
}

// two edges → sharp corners round off. Several passes turn the hull into a soft blob.
export function roundPolygon(points: Point[], iterations = 3): Point[] {
  let pts = points;
  for (let it = 0; it < iterations && pts.length >= 3; it++) {
    const next: Point[] = [];
    for (let i = 0; i < pts.length; i++) {
      const a = pts[i];
      const b = pts[(i + 1) % pts.length];
      next.push({ x: a.x * 0.75 + b.x * 0.25, y: a.y * 0.75 + b.y * 0.25 });
      next.push({ x: a.x * 0.25 + b.x * 0.75, y: a.y * 0.25 + b.y * 0.75 });
    }
    pts = next;
  }
  return pts;
}

// territory boundary, never a polygon with visible corners.
export function smoothClosedPath(rawPoints: Point[]): string {
  const points = roundPolygon(rawPoints, 3);
  const n = points.length;
  if (n === 0) return "";
  if (n < 3) {
    return `M ${points[0].x} ${points[0].y}` + points.slice(1).map((p) => ` L ${p.x} ${p.y}`).join("") + " Z";
  }
  let d = `M ${points[0].x} ${points[0].y}`;
  for (let i = 0; i < n; i++) {
    const p0 = points[(i - 1 + n) % n];
    const p1 = points[i];
    const p2 = points[(i + 1) % n];
    const p3 = points[(i + 2) % n];
    const c1x = p1.x + (p2.x - p0.x) / 6;
    const c1y = p1.y + (p2.y - p0.y) / 6;
    const c2x = p2.x - (p3.x - p1.x) / 6;
    const c2y = p2.y - (p3.y - p1.y) / 6;
    d += ` C ${c1x} ${c1y}, ${c2x} ${c2y}, ${p2.x} ${p2.y}`;
  }
  return d + " Z";
}

export function zoneColor(sectionId: string): string {
  let h = 0;
  for (let i = 0; i < sectionId.length; i++) h = (h * 31 + sectionId.charCodeAt(i)) >>> 0;
  const hue = (h * 137.508) % 360;
  // Muted saturation — distinct hue per section but quiet, not loud.
  return `hsl(${hue.toFixed(1)} 38% 62%)`;
}

// Rounded-rect perimeter samples for v3 zone containers (clockwise from top-left).
export function roundedRectHull(box: Box, radius: number): Point[] {
  const r = Math.max(0, Math.min(radius, box.width / 2, box.height / 2));
  const { x, y, width: w, height: h } = box;
  const pts: Point[] = [];
  const arc = (cx: number, cy: number, start: number, end: number) => {
    for (let i = 0; i <= 4; i++) {
      const a = start + ((end - start) * i) / 4;
      pts.push({ x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r });
    }
  };
  arc(x + r, y + r, Math.PI, Math.PI * 1.5);
  arc(x + w - r, y + r, Math.PI * 1.5, Math.PI * 2);
  arc(x + w - r, y + h - r, 0, Math.PI * 0.5);
  arc(x + r, y + h - r, Math.PI * 0.5, Math.PI);
  return pts;
}

export interface ComputedZone {
  hull: Point[];
  labelAnchor: Point;
}

export function zoneCentroid(boxes: Box[]): Point {
  let x = 0;
  let y = 0;
  for (const b of boxes) {
    x += b.x + b.width / 2;
    y += b.y + b.height / 2;
  }
  return { x: x / boxes.length, y: y / boxes.length };
}

function clipToBisector(poly: Point[], own: Point, other: Point, margin = 0): Point[] {
  if (poly.length < 3) return poly;
  const mx = (own.x + other.x) / 2;
  const my = (own.y + other.y) / 2;
  const nx = other.x - own.x;
  const ny = other.y - own.y;
  // instead of sharing the exact bisector → zones breathe apart.
  const t = margin * Math.hypot(nx, ny);
  const side = (p: Point) => (p.x - mx) * nx + (p.y - my) * ny + t; // <= 0 → own side
  const out: Point[] = [];
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    const sa = side(a);
    const sb = side(b);
    if (sa <= 0) out.push(a);
    if ((sa < 0 && sb > 0) || (sa > 0 && sb < 0)) {
      const t = sa / (sa - sb);
      out.push({ x: a.x + t * (b.x - a.x), y: a.y + t * (b.y - a.y) });
    }
  }
  return out;
}

export function computeZonePolygon(boxes: Box[], ownSite: Point, otherSites: Point[], pad: number = ZONE_PADDING): ComputedZone {
  const hull = convexHull(circleSamples(boxes, pad));
  let poly = hull;
  for (const s of otherSites) poly = clipToBisector(poly, ownSite, s, ZONE_GAP / 2);
  if (poly.length < 3) poly = hull;
  let top = poly[0] ?? { x: 0, y: 0 };
  for (const p of poly) if (p.y < top.y) top = p;
  return { hull: poly, labelAnchor: { x: top.x, y: top.y } };
}
