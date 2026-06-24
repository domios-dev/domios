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

// §32.4 two-axis bounded, content-driven size: a task cell grows in BOTH width and
// height with its WORK VOLUME (body+intent+rationale chars, strides, subtasks,
// links), recomputed ONLY on data mutation, never on zoom. Width snaps to column
// slots so columns stay stable; height carries magnitude and is floored at
// aspectFloorRatio·width so a card is never a hollow letterbox. Heavy work =
// visibly bigger MASS at W0 before any text is readable.
export function taskFootprint(task: Task, children: Task[], s: CanvasLayoutSettings): TaskFootprint {
  const bodyChars = task.body.trim().length;
  const intentChars = task.intent?.trim().length ?? 0;
  const rationaleChars = task.model_rationale?.trim().length ?? 0;
  const strideCount = task.milestones.length;
  const subtaskCount = children.length;
  const childBodyChars = children.reduce((acc, c) => acc + c.body.trim().length, 0);
  const linkCount = task.milestones.reduce((acc, m) => acc + m.links.length, 0);

  const textUnits = Math.sqrt(clamp(bodyChars + intentChars + 0.6 * rationaleChars, 0, 3600) / 180);
  const strideUnits = 1.35 * Math.sqrt(clamp(strideCount, 0, 24));
  const subtaskUnits = 1.6 * Math.sqrt(clamp(subtaskCount, 0, 16)) + Math.sqrt(childBodyChars / 600);
  const volume = 1 + textUnits + strideUnits + subtaskUnits + 0.15 * linkCount;

  const widthSlots = Math.floor(Math.min(3, volume / 3.2));
  const width = clamp(s.taskW + s.taskColStep * widthSlots, s.taskW, s.taskMaxW);
  const rawHeight = s.taskBaseH + 38 * volume + 16 * Math.min(strideCount, 8) + 22 * Math.min(subtaskCount, 5);
  const height = Math.max(clamp(rawHeight, s.taskMinH, s.taskMaxH), Math.round(s.aspectFloorRatio * width));

  return { volume, width, height: Math.round(height) };
}
