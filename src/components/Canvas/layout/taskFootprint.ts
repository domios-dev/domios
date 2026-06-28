import type { Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { lineCount, measureFor } from "../canvasRender/textMetrics";
import { nearContentHeight } from "../canvasRender/cardContent";

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

// Same font drawCard renders the near title with — sizing and rendering share metrics
// so the card height fits the wrapped title (no overflow, no mismatch).
const TITLE_FONT = "600 14px Inter, -apple-system, BlinkMacSystemFont, system-ui, sans-serif";
const measureTitle = measureFor(TITLE_FONT);

export interface TaskFootprint {
  volume: number;
  width: number;
  height: number;
}

// Title width heuristic for the column-snap below; ~7.4px/glyph never under-counts so
// a longer title earns a wider card. The HEIGHT uses real measured wrapping (above).
const TITLE_GLYPH_W = 7.4;
const TITLE_INNER_PAD = 30; // card horizontal padding + border
const SCHEMATIC_TITLE_LINES = 2;

// §32.4 two-axis bounded, content-driven size: a task cell grows in BOTH width and
// height with its WORK VOLUME (body+intent+rationale chars, strides, subtasks,
// links), recomputed ONLY on data mutation, never on zoom. Width snaps to column
// slots so columns stay stable; height carries magnitude and is floored at
// aspectFloorRatio·width so a card is never a hollow letterbox. Heavy work =
// visibly bigger MASS at W0 before any text is readable.
//
// north-star #2: the title is the card's identity and reads IN FULL from W1 up, so
// the footprint also reserves enough WIDTH and HEIGHT for the whole wrapped title —
// a long title earns a wider column (fewer lines) and a height floor that fits the
// title + header chrome, so the reading-band card never clips its own name.
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

  // Width = max of the work-volume width and the width that wraps the title to ~2
  // lines (the schematic clamp). A longer title earns a wider card so common titles
  // fit fully and only the worst-case (very long) names ellipsis.
  const volumeSlots = Math.floor(Math.min(3, volume / 3.6));
  const volumeWidth = s.taskW + s.taskColStep * volumeSlots;
  const titleWidth = (titleChars * TITLE_GLYPH_W) / SCHEMATIC_TITLE_LINES + TITLE_INNER_PAD;
  const width = clamp(Math.max(volumeWidth, titleWidth), s.taskW, s.taskMaxW);

  // Height fits the NEAR card EXACTLY as drawCard renders it: header + wrapped title
  // (≤2 lines, measured with the SAME font) + a tags row + the one-line fields. Routed
  // through the shared FIT metrics (nearContentHeight) = no footprint/render drift.
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
