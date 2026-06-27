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

// Title metrics for the height floor below. The card title renders at ~13.4px world
// (1.03em of the 13px node root); ~7.4px/glyph is a conservative average so the
// estimated line count never UNDER-counts (a short estimate would clip the title).
const TITLE_GLYPH_W = 7.4;
const TITLE_LINE_H = 17; // 1.03em × 1.25 line-height ≈ 16.75, rounded up
const TITLE_INNER_PAD = 30; // card horizontal padding + border + left accent rail
const MAX_TITLE_LINES = 6; // beyond this the W3 document scroll carries the rest

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

  // Width = max of work-volume slots and a title-driven slot: a long title gets a
  // wider column so it wraps to fewer, more readable lines instead of a tall stack.
  const volumeSlots = Math.floor(Math.min(3, volume / 3.6));
  const titleSlots = clamp(Math.floor((titleChars - 34) / 30), 0, 3);
  const widthSlots = clamp(Math.max(volumeSlots, titleSlots), 0, 3);
  const width = clamp(s.taskW + s.taskColStep * widthSlots, s.taskW, s.taskMaxW);

  // Height floor that fits the FULL wrapped title + header chrome (status row +
  // presence) so W1+ never clips the title. Estimated at the chosen width; the body
  // peek/meta allowance keeps the card from feeling cramped under the title.
  const charsPerLine = Math.max(8, Math.floor((width - TITLE_INNER_PAD) / TITLE_GLYPH_W));
  const titleLines = clamp(Math.ceil(titleChars / charsPerLine), 1, MAX_TITLE_LINES);
  const headerH = 24 + titleLines * TITLE_LINE_H + 20; // status/eyebrow + title + presence
  const titleFloorH = headerH + 56; // + meta/summary row peek + padding

  const rawHeight = s.taskBaseH + 24 * volume + 12 * Math.min(strideCount, 8) + 16 * Math.min(subtaskCount, 5);
  const height = Math.max(
    clamp(Math.max(rawHeight, titleFloorH), s.taskMinH, s.taskMaxH),
    Math.round(s.aspectFloorRatio * width),
  );

  return { volume, width, height: Math.round(height) };
}
