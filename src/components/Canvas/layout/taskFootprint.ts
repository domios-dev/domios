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

// Title metrics for the width/height fit below. The card title renders at ~13.4px
// world (1.03em of the 13px node root); ~7.4px/glyph is a conservative average so the
// estimated line count never UNDER-counts (a short estimate would clip the title).
const TITLE_GLYPH_W = 7.4;
const TITLE_LINE_H = 17; // 1.03em × 1.25 line-height ≈ 16.75, rounded up
const TITLE_INNER_PAD = 30; // card horizontal padding + border
// UX-2 schematic face clamps the title to 2 lines, so the footprint targets a 2-line
// fit: WIDTH grows with the title (fewer ellipses on common titles), HEIGHT reserves
// 2 title lines + the one-line description + the glyph row.
const SCHEMATIC_TITLE_LINES = 2;
const DESC_LINE_H = 16; // optional one-line description (0.82em)
const GLYPH_ROW_H = 26; // status · priority · presence · subtasks
const FACE_PAD_H = 36; // top/bottom padding + gaps on the schematic face

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

  // Height floor that fits the SCHEMATIC face — up to 2 title lines + the one-line
  // description + the glyph row — so the title never clips and the card is not a tall
  // letterbox reserving lines the 2-line clamp will never show.
  const charsPerLine = Math.max(8, Math.floor((width - TITLE_INNER_PAD) / TITLE_GLYPH_W));
  const titleLines = clamp(Math.ceil(titleChars / charsPerLine), 1, SCHEMATIC_TITLE_LINES);
  const faceFloorH = titleLines * TITLE_LINE_H + DESC_LINE_H + GLYPH_ROW_H + FACE_PAD_H;

  const rawHeight = s.taskBaseH + 24 * volume + 12 * Math.min(strideCount, 8) + 16 * Math.min(subtaskCount, 5);
  const height = Math.max(
    clamp(Math.max(rawHeight, faceFloorH), s.taskMinH, s.taskMaxH),
    Math.round(s.aspectFloorRatio * width),
  );

  return { volume, width: Math.round(width), height: Math.round(height) };
}
