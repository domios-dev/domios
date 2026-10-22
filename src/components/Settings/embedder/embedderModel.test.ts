import { describe, expect, it } from "vitest";
import { embedderOptions, normalizeRefusal, parseOption, progressLabel, switchSummary } from "./embedderModel";
import { listingFixture, statusFixture } from "./embedderFixtures";


describe("embedder model", () => {
  it.each([
    [new TypeError("fixture transport failure"), "fixture transport failure"],
    ["fixture string failure", "fixture string failure"],
    [{ message: "fixture refusal without hints" }, "fixture refusal without hints"],
  ])("normalizes an error without hints: %j", (reason, message) => {
    expect(normalizeRefusal(reason)).toEqual({ message, hints: [] });
  });

  it("keeps only string hints and supplies a message for unknown errors", () => {
    expect(normalizeRefusal({ message: "fixture failure", hints: ["fixture hint", null, 7, {}] }))
      .toEqual({ message: "fixture failure", hints: ["fixture hint"] });
    for (const reason of [null, undefined, 7, {}, { message: false, hints: "invalid" }]) {
      const refusal = normalizeRefusal(reason);
      expect(typeof refusal.message).toBe("string");
      expect(refusal.message.length).toBeGreaterThan(0);
      expect(refusal.hints).toEqual([]);
    }
  });

  it("disables a remote option while the gate refuses it and round-trips option values", () => {
    const options = embedderOptions(listingFixture(true));
    expect(options.map((option) => [option.provider, option.disabled])).toEqual([["local", false], ["remote-x", true]]);
    expect(options[1].reason).toBe("no key stored");
    expect(parseOption(options[1].value)).toEqual({ provider: "remote-x", model: "wide" });
    expect(embedderOptions(listingFixture(false))[1].disabled).toBe(false);
  });

  it("labels progress and the pre-switch count", () => {
    const running = statusFixture({ reembed: { state: "running", target: null, total: 200, done: 50, failed: 0 } });
    expect(progressLabel(running)).toBe("Re-embedding 50 of 200 (25%)");
    expect(switchSummary({ target: running, remote: true, warning: null, plan: { items: 5, entities: 2, docs: 1 }, total: 8, refusal: null }))
      .toContain("re-embeds 8 items");
  });
});
