export const AUTOMATION_BUTTON = "rounded border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-3 py-2 text-left text-xs text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";
export const AUTOMATION_TOOL = "inline-flex h-8 min-w-8 items-center justify-center gap-1.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-2 text-xs text-[var(--ct-muted-fg)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] disabled:opacity-50";
export const AUTOMATION_ROW = "group flex w-full min-w-0 items-center gap-3 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-2.5 text-left text-[var(--ct-fg)] hover:border-[color-mix(in_srgb,var(--ct-accent)_55%,var(--ct-border))] hover:bg-[var(--ct-hover)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] aria-pressed:border-[var(--ct-accent)]";
export const AUTOMATION_LABEL = "text-[0.6875rem] font-medium uppercase tracking-wider text-[var(--ct-muted-fg)]";
export const AUTOMATION_BODY = "text-[length:var(--ct-font-size-sm)] leading-relaxed";

export function stateTone(state: string): string {
  const value = state.toLowerCase();
  if (["active", "enabled", "running", "working", "success", "completed", "delivered", "done"].includes(value)) return "bg-[var(--ct-green)]";
  if (["missing", "unavailable", "failed", "error", "expired"].includes(value)) return "bg-[var(--ct-red)]";
  if (["paused", "stopped", "idle"].includes(value)) return "bg-[var(--ct-muted)]";
  return "bg-[var(--ct-yellow)]";
}
