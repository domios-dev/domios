import type { Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { lineCount, measureFor } from "../canvasRender/textMetrics";
import { nearContentHeight } from "../canvasRender/cardContent";

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

// so the card height fits the wrapped title (no overflow, no mismatch).
const TITLE_FONT = "600 14px Inter, -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
const measureTitle = measureFor(TITLE_FONT);

export interface TaskFootprint {
  volume: number;
  width: number;
  height: number;
}

// a longer title earns a wider card. The HEIGHT uses real measured wrapping (above).
const TITLE_GLYPH_W = 7.4;
const TITLE_INNER_PAD = 30; // card horizontal padding + border
const SCHEMATIC_TITLE_LINES = 2;

export function taskFootprint(task: Task, children: Task[], s: CanvasLayoutSettings): TaskFootprint {
  const bodyChars = task.body.trim().length;
  const intentChars = task.intent?.trim().length ?? 0;
  const rationaleChars = task.model_rationale?.trim().length ?? 0;
  const strideCount = task.milestones.length;
  const subtaskCount = children.length;
  const childBodyChars = children.reduce((acc, c) => acc + c.body.trim().length, 0);
  const linkCount = task.milestones.reduce((acc, m) => acc + m.links.length, 0);
  const titleChars = task.name.trim().length;

  const textUnits = Math.sqrt(clamp(bodyChars + intentChars + 0.6 * rationaleChars, 0, 3600) / 180);
  const strideUnits = 1.35 * Math.sqrt(clamp(strideCount, 0, 24));
  const subtaskUnits = 1.6 * Math.sqrt(clamp(subtaskCount, 0, 16)) + Math.sqrt(childBodyChars / 600);
  const volume = 1 + textUnits + strideUnits + subtaskUnits + 0.15 * linkCount;

  const volumeSlots = Math.floor(Math.min(3, volume / 3.6));
  const volumeWidth = s.taskW + s.taskColStep * volumeSlots;
  const titleWidth = (titleChars * TITLE_GLYPH_W) / SCHEMATIC_TITLE_LINES + TITLE_INNER_PAD;
  const width = clamp(Math.max(volumeWidth, titleWidth), s.taskW, s.taskMaxW);

  const titleLines = Math.min(2, lineCount(measureTitle, task.name.trim(), width - 24, 2));
  const fields = 3 + (subtaskCount > 0 ? 1 : 0); // status · priority · assignee (· subtasks)
  const contentH = nearContentHeight({ titleLines, hasTags: !!task.tags?.length, sections: 0, fields });

  const rawHeight = s.taskBaseH + 18 * volume + 12 * Math.min(strideCount, 8) + 16 * Math.min(subtaskCount, 5);
  const height = Math.max(
    clamp(Math.max(rawHeight, contentH), s.taskMinH, s.taskMaxH),
    Math.round(s.aspectFloorRatio * width),
  );

  return { volume, width: Math.round(width), height: Math.round(height) };
}
