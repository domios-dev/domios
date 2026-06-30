export const PLAN_VIEW_MODES = ["canvas", "list", "board"] as const;

export type PlanViewMode = (typeof PLAN_VIEW_MODES)[number];

export const DEFAULT_PLAN_VIEW_MODE: PlanViewMode = "canvas";

export const PLAN_VIEW_MODE_LABELS: Record<PlanViewMode, string> = {
  canvas: "Canvas",
  list: "List",
  board: "Board",
};

export function isPlanViewMode(value: string): value is PlanViewMode {
  return (PLAN_VIEW_MODES as readonly string[]).includes(value);
}

export function planViewModeLabel(mode: PlanViewMode): string {
  return PLAN_VIEW_MODE_LABELS[mode];
}

export function resolvePlanViewMode(value: string | null | undefined): PlanViewMode {
  if (value && isPlanViewMode(value)) return value;
  return DEFAULT_PLAN_VIEW_MODE;
}
