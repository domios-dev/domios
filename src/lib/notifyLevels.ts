import type { Notification } from "./types";

/** The three-way axis a notification sits on. Mirrors `NotifyLevel` in
 *  `codeterm-types`: a level outside this set never parses on any transport. */
export type NotifyLevelName = "verbose" | "action_needed" | "failure";

/** Derived from the wire value, so no surface restates the level set. */
export function levelLabel(level: string): string {
  const spaced = level.replace(/_/g, " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/** Existing product tokens, keyed by level. A level the set gains later falls
 *  back to the neutral tone rather than going uncoloured or unrendered. */
const LEVEL_TONE: Record<string, string> = {
  verbose: "var(--ct-muted-fg)",
  action_needed: "var(--ct-yellow)",
  failure: "var(--ct-red)",
};

export function levelTone(level: string): string {
  return LEVEL_TONE[level] ?? "var(--ct-muted-fg)";
}

/** Fallback for records with no declared level (diagnostics, provider-update
 *  prompts): a declared level always wins below; severity is never consulted once one is present. */
const LEVEL_BY_SEVERITY: Record<Notification["severity"], NotifyLevelName> = {
  Info: "verbose",
  Warning: "action_needed",
  Error: "failure",
};

export function notificationLevel(notification: Notification): NotifyLevelName {
  return notification.level ?? LEVEL_BY_SEVERITY[notification.severity];
}

/** The accessible name of one ask, composed from the level and the label the
 *  question carries, so no surface hardcodes what a reader hears. */
export function askAccessibleName(level: string, title: string): string {
  return `${levelLabel(level)}: ${title}`;
}

/** The accessible name of one offered choice, placed inside its question. */
export function choiceAccessibleName(level: string, title: string, choice: string): string {
  return `${choice} — ${askAccessibleName(level, title)}`;
}
