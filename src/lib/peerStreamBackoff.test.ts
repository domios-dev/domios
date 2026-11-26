import { describe, expect, finish, it } from "../test/standaloneTest";
import { computePeerStreamBackoffMs, createPeerStreamBackoff, parseRetryAfterMs } from "./peerStreamBackoff";

describe("peer stream refusal backoff", () => {
  it("grows exponentially with jitter and caps at the maximum delay", () => {
    let clock = 10_000;
    const samples = [0, 1, 0, 1, 0, 1, 0.5];
    let index = 0;
    const backoff = createPeerStreamBackoff({
      now: () => clock,
      random: () => samples[index++] ?? 0.5,
      firstMs: 1_000,
      capMs: 30_000,
      refreshCooldownMs: 10_000,
    });

    const expected = [500, 2_000, 2_000, 8_000, 8_000, 30_000, 22_500];
    for (let attempt = 0; attempt < expected.length; attempt++) {
      const decision = backoff.recordRefusal();
      expect(decision.attempts).toBe(attempt + 1);
      expect(decision.delayMs).toBe(expected[attempt]);
      expect(backoff.isBlocked()).toBe(true);
      expect(backoff.remainingMs()).toBe(expected[attempt]);
      clock += expected[attempt] - 1;
      expect(backoff.isBlocked()).toBe(true);
      expect(backoff.remainingMs()).toBe(1);
      clock += 1;
      expect(backoff.isBlocked()).toBe(false);
      expect(backoff.remainingMs()).toBe(0);
    }
  });

  it("rate-limits credential refresh prompts and resets cleanly on recovery", () => {
    let clock = 5_000;
    const backoff = createPeerStreamBackoff({
      now: () => clock,
      random: () => 1,
      firstMs: 1_000,
      capMs: 30_000,
      refreshCooldownMs: 10_000,
    });

    expect(backoff.recordRefusal().shouldRefresh).toBe(true);
    clock += 2_000;
    expect(backoff.recordRefusal().shouldRefresh).toBe(false);
    clock += 8_000;
    expect(backoff.recordRefusal().shouldRefresh).toBe(true);

    backoff.reset();
    expect(backoff.attempts()).toBe(0);
    expect(backoff.isBlocked()).toBe(false);
    expect(backoff.recordRefusal()).toMatchObject({ delayMs: 1_000, attempts: 1, shouldRefresh: true });
  });

  it("clamps invalid random values into the jitter band", () => {
    expect(computePeerStreamBackoffMs(1, -5, 1_000, 30_000)).toBe(500);
    expect(computePeerStreamBackoffMs(1, 5, 1_000, 30_000)).toBe(1_000);
    expect(computePeerStreamBackoffMs(1, Number.NaN, 1_000, 30_000)).toBe(750);
  });

  it("honors Retry-After floors clamped to the cap", () => {
    expect(parseRetryAfterMs("2")).toBe(2_000);
    expect(parseRetryAfterMs(" 5 ")).toBe(5_000);
    expect(parseRetryAfterMs("0")).toBeUndefined();
    expect(parseRetryAfterMs("bad")).toBeUndefined();
    expect(parseRetryAfterMs(null)).toBeUndefined();

    let clock = 1_000;
    const backoff = createPeerStreamBackoff({
      now: () => clock,
      random: () => 0,
      firstMs: 1_000,
      capMs: 30_000,
      refreshCooldownMs: 10_000,
    });
    expect(backoff.recordRefusal(5_000).delayMs).toBe(5_000);
    clock += 5_000;
    expect(backoff.recordRefusal(120_000).delayMs).toBe(30_000);
  });
});

void finish();
