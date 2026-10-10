export const AUTOMATION_TOOL = "ct-bar-pill disabled:opacity-50";
export const AUTOMATION_PRIMARY = "ct-bar-pill ct-bar-pill--primary disabled:opacity-50";
export const AUTOMATION_ROW = "ct-card group flex w-full min-w-0 items-center gap-3 px-3 py-2.5 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] aria-pressed:border-[var(--ct-accent)]";
export const AUTOMATION_TONE = { cron: "var(--ct-tone-cron)", blueprint: "var(--ct-tone-blueprint)" } as const;
export const AUTOMATION_LABEL = "text-[11.5px] font-semibold text-[var(--ct-muted-fg)]";
export const AUTOMATION_BODY = "text-[length:var(--ct-font-size-sm)] leading-relaxed";

export function stateTone(state: string): string {
  const value = state.toLowerCase();
  if (["active", "enabled", "running", "working", "success", "completed", "delivered", "done"].includes(value)) return "bg-[var(--ct-green)]";
  if (["missing", "unavailable", "failed", "error", "expired"].includes(value)) return "bg-[var(--ct-red)]";
  if (["paused", "stopped", "idle"].includes(value)) return "bg-[var(--ct-muted)]";
  return "bg-[var(--ct-yellow)]";
}
