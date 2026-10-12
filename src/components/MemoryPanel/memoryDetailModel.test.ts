import { describe, expect, it } from "vitest";
import { memoryDetailIdentity, memoryListControlsVisible } from "./memoryDetailModel";

describe("memory detail presentation", () => {
  it("leads with the stored name and retains the identifier", () => {
    expect(memoryDetailIdentity({ title: "Identity", id: "preference_identity" })).toEqual({ title: "Identity", identifier: "preference_identity" });
  });
  it("keeps identifiers secondary when no title is available", () => {
    expect(memoryDetailIdentity({ title: " ", id: "fixture-id" })).toEqual({ title: "Memory item", identifier: "fixture-id" });
    expect(memoryDetailIdentity(null)).toEqual({ title: "Memory item", identifier: null });
  });
  it("hides list controls until the detail returns", () => {
    expect(memoryListControlsVisible(true)).toBe(false);
    expect(memoryListControlsVisible(false)).toBe(true);
  });
});
