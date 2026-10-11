import { describe, expect, it } from "vitest";
import { orderedListGutterCh } from "./orderedListGutter";

describe("ordered list marker gutter", () => {
  it("reserves the complete markers for the audited 75–80 list", () => {
    expect(orderedListGutterCh(75, 6)).toBe(3.5);
  });
  it("accounts for crossing a decimal boundary", () => {
    expect(orderedListGutterCh(1, 100)).toBe(4.5);
  });
  it("accounts for reversed and signed markers", () => {
    expect(orderedListGutterCh(100, 100, true)).toBe(4.5);
    expect(orderedListGutterCh(-10, 2)).toBe(4.5);
  });
  it("bounds invalid starts and empty lists", () => {
    expect(orderedListGutterCh(NaN, 0)).toBe(2.5);
  });
});
