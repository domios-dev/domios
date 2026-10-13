import { describe, expect, it } from "vitest";
import { activeGuideSection } from "./guideScrollModel";

const sections = [
  { id: "overview", top: 100, bottom: 1100 },
  { id: "workspaces", top: 1140, bottom: 2140 },
  { id: "agents", top: 2180, bottom: 2280 },
];

function at(top: number, height = 600, atEnd = false) {
  return activeGuideSection(sections, { top, bottom: top + height, atEnd });
}

describe("activeGuideSection", () => {
  it("keeps a long section selected while reading its body", () => {
    expect(at(800)).toBe("overview");
  });
  it("switches at the heading's scroll margin", () => {
    expect(at(1123)).toBe("overview");
    expect(at(1124)).toBe("workspaces");
  });
  it("keeps the preceding section in the gap between sections", () => {
    expect(at(1110)).toBe("overview");
  });
  it("follows both forward and backward scrolling across three sections", () => {
    expect([100, 1140, 2180, 1140, 100].map((top) => at(top)))
      .toEqual(["overview", "workspaces", "agents", "workspaces", "overview"]);
  });
  it("selects a short final section when the scrollport reaches the end", () => {
    expect(at(1700, 600, true)).toBe("agents");
  });
  it("uses the same heading rule for desktop and iPad viewport heights", () => {
    expect(at(1124, 600)).toBe("workspaces");
    expect(at(1124, 1000)).toBe("workspaces");
  });
  it("selects the first visible section before a heading reaches the reading line", () => {
    expect(at(50)).toBe("overview");
  });
  it("ignores empty and hidden viewports", () => {
    expect(at(100, 0)).toBeNull();
    expect(activeGuideSection([], { top: 0, bottom: 600, atEnd: false })).toBeNull();
  });
});
