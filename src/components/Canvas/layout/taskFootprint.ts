import type { Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

export interface TaskFootprint {
  volume: number;
  width: number;
  height: number;
}

// §26.4 data-driven footprint: a task cell's size ∝ its WORK VOLUME (body chars +
// steps + subtasks + rationale), recomputed ONLY on data mutation, never on zoom.
// The reserved bbox IS the deepest-LOD reserve (§25.2) so content-LOD only repaints
// inside it — heavy tasks are physically BIGGER (mass visible at W0) and footprint
// never changes while zooming; overflow scrolls node-internally at W3.
// Width stays a fixed column slot (settings.taskW) so columns stay stable.
export function taskFootprint(task: Task, children: Task[], s: CanvasLayoutSettings): TaskFootprint {
  const bodyUnits = clamp(Math.ceil(task.body.trim().length / 240), 0, 10);
  const stepCount = task.milestones.length;
  const stepUnits = clamp(stepCount, 0, 12);
  const subtaskUnits = clamp(
    children.reduce(
      (acc, c) => acc + 1 + Math.ceil(c.body.trim().length / 320) + Math.ceil(c.milestones.length / 3),
      0,
    ),
    0,
    18,
  );
  const volume = bodyUnits * 1.0 + stepUnits * 1.4 + subtaskUnits * 2.0 + (task.model_rationale ? 1 : 0);
  const height = clamp(
    Math.round(s.taskBaseH + 34 * Math.sqrt(volume) * Math.sqrt(1 + stepCount / 4) + 38 * Math.min(children.length, 6)),
    s.taskBaseH,
    s.taskMaxH,
  );
  return { volume, width: s.taskW, height };
}
