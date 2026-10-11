export const CHAT_ERROR_TOAST_LIFETIME_MS = 10_000;

export const MAX_TOAST_LIFETIME_MS = 15_000;

export function toastLifetime(configured = 5000): number {
  return Number.isFinite(configured) ? Math.max(1, Math.min(MAX_TOAST_LIFETIME_MS, configured)) : 5000;
}

export function remainingLifetime(remaining: number, startedAt: number, now: number): number {
  return Math.max(0, remaining - Math.max(0, now - startedAt));
}

export function countdownAnimation(reducedMotion: boolean): string {
  return reducedMotion ? "none" : "ct-toast-countdown";
}
