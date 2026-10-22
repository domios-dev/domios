import { describe, expect, it } from "vitest";
import { embedderOptions, parseOption, progressLabel, switchSummary } from "./embedderModel";
import { listingFixture, statusFixture } from "./embedderFixtures";


describe("embedder model", () => {
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
