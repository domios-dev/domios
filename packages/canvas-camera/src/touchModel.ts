import type { ViewportTransform } from "./viewportModel";

export interface CameraPoint { x: number; y: number }

export function touchTransform(base: ViewportTransform, before: CameraPoint[], after: CameraPoint[], min: number, max: number): ViewportTransform {
  const midpoint = (points: CameraPoint[]) => points.length > 1
    ? { x: (points[0].x + points[1].x) / 2, y: (points[0].y + points[1].y) / 2 } : points[0];
  const a = midpoint(before);
  const b = midpoint(after);
  if (!a || !b) return base;
  const distance = (points: CameraPoint[]) => Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y);
  const initialDistance = before.length > 1 ? distance(before) : 0;
  const scale = initialDistance > 0 && after.length > 1
    ? Math.max(min, Math.min(max, base.scale * distance(after) / initialDistance)) : base.scale;
  const ratio = scale / base.scale;
  return { scale, x: b.x - (a.x - base.x) * ratio, y: b.y - (a.y - base.y) * ratio };
}
