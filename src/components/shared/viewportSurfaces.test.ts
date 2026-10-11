import { describe, expect, it } from "vitest";
import { anchoredFloatingPosition } from "./floatingPosition";
import { anchoredPanelPosition } from "../Notifications/notificationPanelPosition";

describe("viewport surface bounds", () => {
  it.each([[390, 844], [844, 390]])("clamps both menu alignments at %ix%i", (width, height) => {
    for (const align of ["left", "right"] as const) {
      for (const rect of [
        { left: 12, right: 195.16, bottom: 162 },
        { left: width - 20, right: width, bottom: height - 10 },
      ]) {
        const pos = anchoredFloatingPosition(rect, 248, 300, width, height, align);
        expect(pos.left).toBeGreaterThanOrEqual(4);
        expect(pos.left + 248).toBeLessThanOrEqual(width - 4);
        expect(pos.top).toBeGreaterThanOrEqual(4);
        expect(pos.top + Math.min(300, pos.maxHeight)).toBeLessThanOrEqual(height - 4);
      }
    }
  });

  it("preserves a menu's desktop anchor", () => {
    expect(anchoredFloatingPosition({ left: 1000, right: 1100, bottom: 44 }, 248, 200, 1600, 900, "right"))
      .toEqual({ left: 852, top: 48, maxHeight: 848 });
  });

  it.each([200, 390, 844, 1600])("fits the bell panel with every anchor at width %i", (width) => {
    for (const rightEdge of [0, 40, width / 2, width]) {
      const pos = anchoredPanelPosition({ right: rightEdge, bottom: 44 }, width);
      expect(pos.right).toBeGreaterThanOrEqual(8);
      expect(width - pos.right - pos.maxWidth).toBeGreaterThanOrEqual(8);
      expect(pos.maxWidth).toBeGreaterThanOrEqual(Math.min(240, width - 16));
    }
  });

  it("preserves the desktop bell anchor and expanded reading room", () => {
    expect(anchoredPanelPosition({ right: 1500, bottom: 44 }, 1600))
      .toEqual({ top: 50, right: 100, maxWidth: 1492 });
  });
});
