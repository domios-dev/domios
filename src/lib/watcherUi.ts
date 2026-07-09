export type WatcherStatusTone = "neutral" | "ok" | "warn" | "danger";

export function watcherStatusTone(status: string | null | undefined): WatcherStatusTone {
  switch ((status ?? "").trim().toLowerCase()) {
    case "ok":
      return "ok";
    case "attention":
      return "warn";
    case "stalled":
      return "danger";
    default:
      return "neutral";
  }
}

export function watcherStatusLabel(status: string | null | undefined): string {
  const key = (status ?? "").trim().toLowerCase();
  if (key === "ok") return "OK";
  if (key === "attention") return "ATTENTION";
  if (key === "stalled") return "STALLED";
  return "WATCHER";
}

export const WATCHER_TONE_STYLES: Record<
  WatcherStatusTone,
  { color: string; background: string; borderColor: string }
> = {
  neutral: {
    color: "var(--ct-muted-fg, #8b949e)",
    background: "var(--ct-surface, #161b22)",
    borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
  },
  ok: {
    color: "var(--ct-green, #3fb950)",
    background: "color-mix(in srgb, var(--ct-surface, #161b22) 90%, var(--ct-green, #3fb950) 10%)",
    borderColor: "color-mix(in srgb, var(--ct-green, #3fb950) 32%, transparent)",
  },
  warn: {
    color: "var(--ct-yellow, #d29922)",
    background: "color-mix(in srgb, var(--ct-surface, #161b22) 90%, var(--ct-yellow, #d29922) 10%)",
    borderColor: "color-mix(in srgb, var(--ct-yellow, #d29922) 32%, transparent)",
  },
  danger: {
    color: "var(--ct-red, #f85149)",
    background: "color-mix(in srgb, var(--ct-surface, #161b22) 90%, var(--ct-red, #f85149) 10%)",
    borderColor: "color-mix(in srgb, var(--ct-red, #f85149) 32%, transparent)",
  },
};

export const CHARTER_PREVIEW_CHARS = 120;

export function charterPreview(charter: string, expanded: boolean): string {
  if (expanded || charter.length <= CHARTER_PREVIEW_CHARS) return charter;
  return `${charter.slice(0, CHARTER_PREVIEW_CHARS).trimEnd()}…`;
}
