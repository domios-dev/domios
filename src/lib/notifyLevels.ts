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

/** A record whose sender named no level makes no claim about urgency, so every
 *  surface paints it neutral by this name — never by a level invented for it. */
export const UNLEVELLED_TONE = "var(--ct-muted-fg)";

export function levelTone(level: string): string {
  return LEVEL_TONE[level] ?? UNLEVELLED_TONE;
}

/** What each level claims, in the vocabulary the settings design uses. A level
 *  the set gains has nothing to claim here until someone writes it, so it gets
 *  no line rather than a guessed one. */
const LEVEL_MEANING: Record<string, string> = {
  verbose: "Progress, nothing waiting",
  action_needed: "The agent is waiting on you",
  failure: "It broke and the agent stopped",
};

export function levelMeaning(level: string): string | null {
  return LEVEL_MEANING[level] ?? null;
}

/** The record's own level, or `null` when it carries none. `severity` is a
 *  separate wire axis and is never read here: tone must agree with routing. */
export function notificationLevel(notification: Notification): NotifyLevelName | null {
  return notification.level ?? null;
}

/** The one tone authority. Every notification surface paints from this, so one
 *  record can never read calm on the bell and urgent in the transcript. */
export function notificationTone(notification: Notification): string {
  const level = notificationLevel(notification);
  return level === null ? UNLEVELLED_TONE : levelTone(level);
}

/** The accessible name of one ask, composed from the level and the label the
 *  question carries, so no surface hardcodes what a reader hears. An unlevelled
 *  record is named by its label alone rather than by a level it never claimed. */
export function askAccessibleName(level: string | null, title: string): string {
  return level ? `${levelLabel(level)}: ${title}` : title;
}

/** The accessible name of one offered choice, placed inside its question. */
export function choiceAccessibleName(level: string | null, title: string, choice: string): string {
  return `${choice} — ${askAccessibleName(level, title)}`;
}
