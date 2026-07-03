export const CANVAS_SUB_MODES = ["mission-control", "section-graph"] as const;

export type CanvasSubMode = (typeof CANVAS_SUB_MODES)[number];

export const DEFAULT_CANVAS_SUB_MODE: CanvasSubMode = "mission-control";

export const CANVAS_SUB_MODE_LABELS: Record<CanvasSubMode, string> = {
  "mission-control": "Mission control",
  "section-graph": "Section graph",
};

export function isCanvasSubMode(value: string): value is CanvasSubMode {
  return (CANVAS_SUB_MODES as readonly string[]).includes(value);
}

export function resolveCanvasSubMode(value: string | null | undefined): CanvasSubMode {
  if (value && isCanvasSubMode(value)) return value;
  return DEFAULT_CANVAS_SUB_MODE;
}
