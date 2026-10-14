import { describe, expect, it } from "vitest";
import { capabilityHintsEnabled, capabilityHintsPatch } from "./generalAgentSettingsModel";

describe("General Agent capability hints setting", () => {
  it("defaults on when missing", () => {
    expect(capabilityHintsEnabled(undefined)).toBe(true);
    expect(capabilityHintsEnabled({})).toBe(true);
  });
  it("preserves both user choices in the config patch", () => {
    for (const enabled of [false, true]) {
      const patch = capabilityHintsPatch(enabled);
      expect(patch).toEqual({ capability_hints: enabled });
      expect(capabilityHintsEnabled(patch)).toBe(enabled);
    }
  });
});
