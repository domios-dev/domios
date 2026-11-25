import { describe, expect, it } from "vitest";
import { chatRowOffsets, mountedChatWindow, mountedChatWindowCoversViewport } from "./chatMountedWindow";
import { IndexedChatRowGeometry } from "./chatRowGeometry";

describe("measured chat mounting", () => {
  it("mounts one viewport of overscan and preserves the measured extent", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(100), scrollTop: 8000, viewportHeight: 500 });
    expect(window).toEqual({ start: 75, end: 90, top: 7500, bottom: 13000, total: 22000 });
  });
  it("bounds dense rows to sixty while including the viewport", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(10), scrollTop: 800, viewportHeight: 500 });
    expect(window.end - window.start).toBe(60);
    expect(window.start).toBeLessThanOrEqual(80);
    expect(window.end).toBeGreaterThanOrEqual(130);
  });
  it("caps overscan at the normal target", () => {
    const window = mountedChatWindow({ heights: Array(220).fill(20), scrollTop: 800, viewportHeight: 500 });
    expect(window.end - window.start).toBe(40);
    expect(window.start).toBeLessThanOrEqual(40);
    expect(window.end).toBeGreaterThanOrEqual(65);
  });
  it("includes the live tail and mounts an explicit older target", () => {
    const heights = Array(220).fill(120);
    expect(mountedChatWindow({ heights, scrollTop: 0, viewportHeight: 500, pinned: true }).end).toBe(220);
    const target = mountedChatWindow({ heights, scrollTop: 20000, viewportHeight: 500, targetIndex: 30 });
    expect(target.start).toBeLessThanOrEqual(30);
    expect(target.end).toBeGreaterThan(30);
  });
  it("drops every rich row for an inactive warm chat", () => {
    expect(mountedChatWindow({ heights: [100, 300, 50], scrollTop: 100, viewportHeight: 200, active: false }))
      .toEqual({ start: 0, end: 0, top: 0, bottom: 450, total: 450 });
  });
  it("uses complete row heights including rails, keeps empty rows at zero and estimates invalid measurements", () => {
    expect(chatRowOffsets([100, 450, 200, 0, -1, NaN])).toEqual([0, 100, 550, 750, 750, 870, 990]);
    const window = mountedChatWindow({ heights: [100, 450, 200, 80], scrollTop: 400, viewportHeight: 100 });
    expect(window.start).toBe(1);
    expect(window.top).toBe(100);
    expect(window.bottom).toBe(80);
  });
  it("recognises when the current mounted slice still covers the scrolled viewport with margin", () => {
    const geometry = new IndexedChatRowGeometry();
    for (let index = 0; index < 300; index++) geometry.append(100);
    const window = mountedChatWindow({ heights: Array(300).fill(100), scrollTop: 15000, viewportHeight: 500 });
    expect(mountedChatWindowCoversViewport(window, geometry, 15000, 500)).toBe(true);
    expect(mountedChatWindowCoversViewport(window, geometry, 14800, 500)).toBe(true);
    expect(mountedChatWindowCoversViewport(window, geometry, 14400, 500)).toBe(false);
    expect(mountedChatWindowCoversViewport(window, geometry, 16000, 500)).toBe(false);
  });
});
