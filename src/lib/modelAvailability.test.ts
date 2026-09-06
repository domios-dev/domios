// Run: node node_modules/tsx/dist/cli.mjs src/lib/modelAvailability.test.ts

import { describe, expect, finish, it } from "../test/standaloneTest";
import { isModelSelectable, modelAvailability } from "./modelAvailability";

/**
 * The picker and `codeterm agent models` read one wire field with one
 * precedence. `cli/src/cmd/agent.rs::availability_marker` implements the same
 * table; this proves the frontend half, so the two surfaces cannot disagree.
 */
describe("modelAvailability", () => {
  it("reads the provider's explicit verdict", () => {
    expect(modelAvailability({ availability: "available" })).toBe("available");
    expect(modelAvailability({ availability: "unavailable" })).toBe("unavailable");
    expect(modelAvailability({ availability: "unknown" })).toBe("unknown");
  });

  it("prefers the explicit verdict over the legacy boolean", () => {
    expect(modelAvailability({ available: true, availability: "unknown" })).toBe("unknown");
    expect(modelAvailability({ available: false, availability: "available" })).toBe("available");
  });

  it("falls back to the legacy boolean when nothing probed the model", () => {
    expect(modelAvailability({ available: false })).toBe("unavailable");
    expect(modelAvailability({ available: true })).toBe("available");
    expect(modelAvailability({})).toBe("available");
  });

  it("never turns a could-not-run probe into unavailable", () => {
    expect(modelAvailability({ availability: "unknown" }) === "unavailable").toBe(false);
    expect(isModelSelectable({ availability: "unknown" })).toBe(true);
  });

  it("blocks only a model the provider answered no about", () => {
    expect(isModelSelectable({ availability: "unavailable" })).toBe(false);
    expect(isModelSelectable({ available: false })).toBe(false);
    expect(isModelSelectable({ availability: "available" })).toBe(true);
  });
});

finish();
