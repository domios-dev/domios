const FLAG_KEY = "canvas.overlayBudget";

let cached: boolean | null = null;

export function overlayBudgetEnabled(): boolean {
  if (cached !== null) return cached;
  if (typeof window === "undefined") {
    cached = false;
    return cached;
  }
  try {
    cached = window.localStorage.getItem(FLAG_KEY) === "true";
  } catch {
    cached = false;
  }
  return cached;
}

export const OVERLAY_BUDGET_FLAG_KEY = FLAG_KEY;

export function __resetOverlayBudgetForTest(): void {
  cached = null;
}
