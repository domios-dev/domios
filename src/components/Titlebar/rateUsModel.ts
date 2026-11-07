/**
 * When to invite the user to star Domios on GitHub.
 *
 * Eligibility: first seen at least 24 h ago AND real activity on at least two
 * distinct local days. Two separate days of use say the user came back, and it
 * costs at most two writes, with no timer persisting usage time every minute.
 *
 * The invite shows once. "Later" snoozes it for 7 days and lets it show once
 * more; a second "Later", "Star on GitHub" or "Don't ask again" ends it for good.
 */

export const DAY_MS = 24 * 60 * 60 * 1000;
export const SNOOZE_MS = 7 * DAY_MS;
/** How many "Later"s still allow another showing. */
const MAX_SNOOZES = 1;
/** Two distinct days are all eligibility needs, so the stored list never grows past that. */
const DAYS_KEPT = 2;

/** Persisted under `ui.rate_us` in config. TOML has no null, so absent values are 0. */
export interface RateUsState {
  first_seen: number;
  days: string[];
  snoozes: number;
  snooze_until: number;
  done: boolean;
}

export type RateUsAction = "star" | "later" | "never";

export const INITIAL_RATE_US: RateUsState = { first_seen: 0, days: [], snoozes: 0, snooze_until: 0, done: false };

/** The local calendar day of `now`, so "two days" means two days on the user's own clock. */
export function localDayKey(now: number): string {
  const date = new Date(now);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/** Reads whatever config holds, falling back field by field so a damaged entry never throws. */
export function parseRateUs(value: unknown): RateUsState {
  if (!value || typeof value !== "object") return INITIAL_RATE_US;
  const raw = value as Partial<Record<keyof RateUsState, unknown>>;
  const number = (field: unknown) => typeof field === "number" && Number.isFinite(field) && field > 0 ? field : 0;
  return {
    first_seen: number(raw.first_seen),
    days: Array.isArray(raw.days) ? [...new Set(raw.days.filter((day): day is string => typeof day === "string"))].slice(-DAYS_KEPT) : [],
    snoozes: number(raw.snoozes),
    snooze_until: number(raw.snooze_until),
    done: raw.done === true,
  };
}

/** Notes real use at `now`; returns the same object when nothing new was learned, so callers skip the write. */
export function recordActivity(state: RateUsState, now: number, day: string = localDayKey(now)): RateUsState {
  if (state.done) return state;
  const knownDay = state.days.includes(day);
  if (state.first_seen > 0 && (knownDay || state.days.length >= DAYS_KEPT)) return state;
  return {
    ...state,
    first_seen: state.first_seen || now,
    days: knownDay ? state.days : [...state.days, day].slice(-DAYS_KEPT),
  };
}

/** True when the user has used Domios long enough and the invite is neither finished nor snoozed. */
export function isRateUsDue(state: RateUsState, now: number): boolean {
  if (state.done || state.first_seen === 0) return false;
  if (now - state.first_seen < DAY_MS) return false;
  if (state.days.length < DAYS_KEPT) return false;
  return now >= state.snooze_until;
}

/** The state after the user answers the invite. */
export function answerRateUs(state: RateUsState, action: RateUsAction, now: number): RateUsState {
  if (action === "later" && state.snoozes < MAX_SNOOZES) {
    return { ...state, snoozes: state.snoozes + 1, snooze_until: now + SNOOZE_MS };
  }
  return { ...state, snoozes: action === "later" ? state.snoozes + 1 : state.snoozes, done: true };
}

/** Never covers what the user is typing into: an editable element under the invite keeps it hidden. */
export function overlaps(a: { left: number; top: number; right: number; bottom: number }, b: { left: number; top: number; right: number; bottom: number }): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}
