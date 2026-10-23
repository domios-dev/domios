import { describe, expect, it } from "vitest";
import { embedderOptions, normalizeRefusal, parseOption, progressLabel, refusalForPlatform, switchSummary } from "./embedderModel";
import { listingFixture, statusFixture } from "./embedderFixtures";


describe("embedder model", () => {
  it.each([
    ["Win32", "icacls", "chmod 600"],
    ["Windows", "icacls", "chmod 600"],
    ["MacIntel", "chmod 600", "icacls"],
    ["Linux x86_64", "chmod 600", "icacls"],
    ["Darwin", "chmod 600", "icacls"],
  ])("selects setup hints for %s", (platform, expected, excluded) => {
    const provider = listingFixture(true).providers[1];
    const result = refusalForPlatform(provider.refusal, provider, true, platform);
    expect(result.message).toBe(provider.refusal?.message);
    expect(result.hints[0]).toContain(`domios mem embedder key set ${provider.id}`);
    expect(result.hints.join("\n")).toContain(expected);
    expect(result.hints.join("\n")).not.toContain(excluded);
    expect(result.hints.join("\n")).not.toContain("memory.embedder.remote_opt_in");
  });

  it("offers opt-in only when missing and omits key setup when a usable key is stored", () => {
    const provider = listingFixture(true).providers[1];
    provider.key = { file: { path: "fixture/remote-x-embedder.key", present: false, owner_only: null }, in_vault: true };
    expect(refusalForPlatform(provider.refusal, provider, true, "Win32").hints).toEqual([]);
    const hints = refusalForPlatform(provider.refusal, provider, false, "Win32").hints;
    expect(hints).toHaveLength(1);
    expect(hints[0]).toContain("domios config set memory.embedder.remote_opt_in true");
    provider.key = { ...provider.key, file: { ...provider.key.file, present: true, owner_only: false } };
    expect(refusalForPlatform(provider.refusal, provider, true, "Win32").hints[0])
      .toContain(`domios mem embedder key set ${provider.id}`);
  });

  it("preserves normalized non-provider hints", () => {
    expect(refusalForPlatform({ message: "fixture failure", hints: ["fixture hint", null] }, undefined, true, "Win32"))
      .toEqual({ message: "fixture failure", hints: ["fixture hint"] });
  });

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
    const target = { provider: "remote-x", model: "wide", dimension: 8 };
    expect(progressLabel(statusFixture({ reembed: { state: "running", target, total: 200, done: 50, failed: 0 } })))
      .toContain("remote-x/wide");
    for (const state of ["failed", "cancelled"] as const) {
      const label = progressLabel(statusFixture({ reembed: { state, target, total: 9, done: 4, failed: 1, last_error: "fixture reason" } }));
      expect(label).toContain(state);
      expect(label).toContain("fixture reason");
      expect(label).not.toBe("Index up to date");
    }
    expect(switchSummary({ target: running, remote: true, warning: null, plan: { items: 5, entities: 2, docs: 1 }, total: 8, refusal: null }))
      .toContain("re-embeds 8 items");
  });
});
