import { describe, expect, it } from "vitest";
import { answerRateUs, DAY_MS, INITIAL_RATE_US, isRateUsDue, overlaps, parseRateUs, recordActivity, SNOOZE_MS, type RateUsState } from "./rateUsModel";

const T0 = Date.UTC(2026, 10, 2, 9, 0);
const used = (first: number, ...days: string[]): RateUsState => ({ ...INITIAL_RATE_US, first_seen: first, days });

describe("rate-us eligibility", () => {
  it("records the first sighting and each new day, and stops learning after two days", () => {
    const first = recordActivity(INITIAL_RATE_US, T0, "d1");
    expect(first).toEqual(used(T0, "d1"));
    expect(recordActivity(first, T0 + 1000, "d1")).toBe(first);
    const second = recordActivity(first, T0 + DAY_MS, "d2");
    expect(second).toEqual(used(T0, "d1", "d2"));
    expect(recordActivity(second, T0 + 2 * DAY_MS, "d3")).toBe(second);
  });

  it("is due only after 24 h since first seen and activity on two distinct days", () => {
    expect(isRateUsDue(INITIAL_RATE_US, T0)).toBe(false);
    expect(isRateUsDue(used(T0, "d1"), T0 + 3 * DAY_MS)).toBe(false);
    expect(isRateUsDue(used(T0, "d1", "d2"), T0 + DAY_MS - 1)).toBe(false);
    expect(isRateUsDue(used(T0, "d1", "d2"), T0 + DAY_MS)).toBe(true);
  });

  it("Later snoozes for seven days and allows exactly one more showing", () => {
    const due = used(T0, "d1", "d2");
    const now = T0 + 2 * DAY_MS;
    const snoozed = answerRateUs(due, "later", now);
    expect(isRateUsDue(snoozed, now + SNOOZE_MS - 1)).toBe(false);
    expect(isRateUsDue(snoozed, now + SNOOZE_MS)).toBe(true);
    const again = answerRateUs(snoozed, "later", now + SNOOZE_MS);
    expect(again.done).toBe(true);
    expect(isRateUsDue(again, now + 10 * SNOOZE_MS)).toBe(false);
  });

  it("Star and Don't ask again end it permanently, even from a snooze", () => {
    const due = used(T0, "d1", "d2");
    for (const action of ["star", "never"] as const) {
      expect(isRateUsDue(answerRateUs(due, action, T0 + 2 * DAY_MS), T0 + 100 * DAY_MS)).toBe(false);
      const snoozed = answerRateUs(due, "later", T0 + 2 * DAY_MS);
      expect(answerRateUs(snoozed, action, T0 + 9 * DAY_MS).done).toBe(true);
    }
    const done = answerRateUs(due, "never", T0);
    expect(recordActivity(done, T0 + DAY_MS, "d9")).toBe(done);
  });

  it("parses a damaged or foreign entry into a safe state", () => {
    expect(parseRateUs(undefined)).toEqual(INITIAL_RATE_US);
    expect(parseRateUs("nope")).toEqual(INITIAL_RATE_US);
    expect(parseRateUs({ first_seen: -5, days: ["a", 3, "a", "b", "c"], snoozes: "x", snooze_until: Infinity, done: "yes" }))
      .toEqual({ first_seen: 0, days: ["b", "c"], snoozes: 0, snooze_until: 0, done: false });
    const stored = answerRateUs(used(T0, "d1", "d2"), "later", T0);
    expect(parseRateUs(JSON.parse(JSON.stringify(stored)))).toEqual(stored);
  });

  it("tells when an input sits under the invite's corner", () => {
    const corner = { left: 700, right: 1000, top: 0, bottom: 200 };
    expect(overlaps({ left: 800, right: 900, top: 50, bottom: 70 }, corner)).toBe(true);
    expect(overlaps({ left: 0, right: 600, top: 700, bottom: 740 }, corner)).toBe(false);
  });
});
