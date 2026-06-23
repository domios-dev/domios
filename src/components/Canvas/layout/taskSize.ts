import type { Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";

const CHARS_PER_LINE = 34;
const LINE_H = 19;

function textLines(text: string): number {
  const t = text.trim();
  if (!t) return 0;
  return t.split("\n").reduce((acc, line) => acc + Math.max(1, Math.ceil(line.length / CHARS_PER_LINE)), 0);
}

// Deterministic content-height estimate so the layout reserves enough room and
// task cards never overlap or clip (no DOM measurement, no manual drag).
export function estimateTaskHeight(task: Task, s: CanvasLayoutSettings): number {
  let h = 16 + 24 + 12; // top pad + name/status row + bottom pad
  if (task.model) h += 6;
  if (task.assigned_pane_id) h += 24;
  if (task.model_rationale) h += 22;
  if (task.intent) h += 8 + textLines(task.intent) * LINE_H;
  if (task.body.trim()) h += 8 + textLines(task.body) * LINE_H;
  return Math.max(s.taskMinHeight, h);
}
