// List and Canvas stay implemented, but are parked from the switcher.
export const PLAN_VIEW_MODES = ["board", "spec", "plan", "tasks"] as const;

export type ActivePlanViewMode = (typeof PLAN_VIEW_MODES)[number];
export type ParkedPlanViewMode = "list" | "canvas";
export type PlanViewMode = ActivePlanViewMode | ParkedPlanViewMode;

export const PARKED_PLAN_VIEW_MODES = ["list", "canvas"] as const;

export const DEFAULT_PLAN_VIEW_MODE: PlanViewMode = "board";

export const PLAN_VIEW_MODE_LABELS: Record<PlanViewMode, string> = {
  canvas: "Canvas",
  list: "List",
  board: "Board",
  plan: "Plan",
  spec: "Spec",
  tasks: "Tasks",
};

export function isPlanViewMode(value: string): value is PlanViewMode {
  return [...PLAN_VIEW_MODES, ...PARKED_PLAN_VIEW_MODES].includes(value as PlanViewMode);
}

export function planViewModeLabel(mode: PlanViewMode): string {
  return PLAN_VIEW_MODE_LABELS[mode];
}

export function resolvePlanViewMode(value: string | null | undefined): PlanViewMode {
  if (value && isPlanViewMode(value) && !PARKED_PLAN_VIEW_MODES.includes(value as ParkedPlanViewMode)) {
    return value;
  }
  return DEFAULT_PLAN_VIEW_MODE;
}
