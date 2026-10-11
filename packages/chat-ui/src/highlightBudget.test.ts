import { describe, expect, it } from "vitest";
import { highlightSourceEligible, highlightSpanCount, reserveHighlightSpans, type HighlightReservations } from "./highlightBudget";

describe("chat highlight budget", () => {
  it("limits initial highlighting by both lines and UTF-8 bytes", () => {
    expect(highlightSourceEligible(Array(200).fill("x").join("\n"))).toBe(true);
    expect(highlightSourceEligible(Array(201).fill("x").join("\n"))).toBe(false);
    expect(highlightSourceEligible("x".repeat(16384))).toBe(true);
    expect(highlightSourceEligible("λ".repeat(8193))).toBe(false);
    expect(highlightSourceEligible("λ".repeat(20000), true)).toBe(true);
  });
  it("counts nested spans and line wrappers rather than just outer tokens", () => {
    expect(highlightSpanCount([{ type: "element", tagName: "span", children: [
      { type: "element", tagName: "span", children: [{ type: "text" }] },
      { type: "element", tagName: "div", children: [{ type: "element", tagName: "span" }] },
    ] }])).toBe(3);
  });
  it("enforces 2000 spans per row across hundreds of short fences", () => {
    let current: HighlightReservations = new Map();
    for (let i = 0; i < 100; i += 1) current = reserveHighlightSpans(current, `f${i}`, "one-row", 100);
    expect(current.size).toBe(20);
    expect(reserveHighlightSpans(current, "oversized", "other", 2001).has("oversized")).toBe(false);
  });
  it("enforces the chat cap and accounts for replacement and unmount", () => {
    let current = reserveHighlightSpans(new Map(), "a", "r1", 2000);
    current = reserveHighlightSpans(current, "b", "r2", 2000);
    current = reserveHighlightSpans(current, "c", "r3", 1000);
    expect(reserveHighlightSpans(current, "d", "r4", 1).has("d")).toBe(false);
    current = reserveHighlightSpans(current, "a", "r1", 1500);
    expect(reserveHighlightSpans(current, "a", "r1", 1500)).toBe(current);
    current = reserveHighlightSpans(current, "d", "r4", 500);
    expect(current.get("d")?.spans).toBe(500);
    current = reserveHighlightSpans(current, "b", "r2", 0);
    expect(reserveHighlightSpans(current, "e", "r5", 2000).has("e")).toBe(true);
  });
  it("rejects non-finite or negative work without consuming capacity", () => {
    const current = new Map();
    expect(reserveHighlightSpans(current, "a", "row", Infinity)).toBe(current);
    expect(reserveHighlightSpans(current, "a", "row", -1)).toBe(current);
  });
});
