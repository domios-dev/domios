import { describe, expect, it } from "vitest";
import { TETHER_BEND, TETHER_GAP, TETHER_OWNER_GAP, tetherClearings, tetherPaths, type TetherRect } from "./tetherGeometry";

const owner: TetherRect = { left: 16, top: 8, width: 12, height: 12 };
const ownerOrigin = [22, 20 + TETHER_OWNER_GAP];

const SINGLE_CURVE = /^M(-?[\d.]+) (-?[\d.]+)C(-?[\d.]+) (-?[\d.]+) (-?[\d.]+) (-?[\d.]+) (-?[\d.]+) (-?[\d.]+)$/;

function curve(d: string): number[] {
  const match = SINGLE_CURVE.exec(d);
  expect(match, `not a single M…C… curve: ${d}`).not.toBeNull();
  return match!.slice(1).map(Number);
}

describe("tether geometry", () => {
  it("is a single cubic from just below the owner icon to the attachment icon centre", () => {
    const [d] = tetherPaths(owner, [{ left: 36, top: 34, width: 10, height: 10 }]);
    const [x0, y0, c1x, c1y, c2x, c2y, x1, y1] = curve(d);
    expect([x0, y0]).toEqual(ownerOrigin);
    expect(y0 - (owner.top + owner.height)).toBeGreaterThanOrEqual(2);
    expect(y0 - (owner.top + owner.height)).toBeLessThanOrEqual(3);
    expect([x1, y1]).toEqual([41, 39]);
    expect([c1x, c1y]).toEqual([22, y0 + (39 - y0) * TETHER_BEND]);
    expect([c2x, c2y]).toEqual([41 - 19 * TETHER_BEND, 39]);
  });

  it("never contains straight segments, so it cannot read as a tree corner", () => {
    const paths = tetherPaths(owner, [
      { left: 36, top: 34, width: 10, height: 10 },
      { left: 17, top: 34, width: 10, height: 10 },
      { left: 4, top: 60, width: 10, height: 10 },
    ]);
    for (const d of paths) {
      expect(d).not.toMatch(/[LHVQSAlhvqsa]/);
      expect(d.match(/C/g)).toHaveLength(1);
    }
  });

  it("fans one curve per attachment from the same owner origin", () => {
    const paths = tetherPaths(owner, [
      { left: 36, top: 34, width: 10, height: 10 },
      { left: 36, top: 54, width: 10, height: 10 },
    ]);
    expect(paths).toHaveLength(2);
    const [first, second] = paths.map(curve);
    expect(first.slice(0, 2)).toEqual(ownerOrigin);
    expect(second.slice(0, 2)).toEqual(ownerOrigin);
    expect(second.slice(6)).toEqual([41, 59]);
    expect(second[3]).toBeGreaterThan(first[3]);
  });

  it("never draws upward or from unmeasured boxes", () => {
    expect(tetherPaths(owner, [{ left: 36, top: 0, width: 10, height: 10 }])).toEqual([""]);
    expect(tetherPaths(owner, [{ left: 36, top: 13, width: 10, height: 10 }])).toEqual([""]);
    expect(tetherPaths(owner, [{ left: 0, top: 0, width: 0, height: 0 }])).toEqual([""]);
    expect(tetherPaths({ left: 0, top: 0, width: 0, height: 0 }, [{ left: 36, top: 34, width: 10, height: 10 }])).toEqual([""]);
  });

  it("rounds coordinates so sub-pixel layout does not churn the path", () => {
    const [d] = tetherPaths({ left: 16.333, top: 8.111, width: 12, height: 12 }, [{ left: 36.6666, top: 34, width: 10, height: 10 }]);
    expect(d).toBe("M22.33 22.61C22.33 30.81 32 39 41.67 39");
  });

  it("clears a disc around every measured attachment icon", () => {
    expect(tetherClearings([{ left: 36, top: 34, width: 10, height: 10 }, { left: 0, top: 0, width: 0, height: 0 }])).toEqual([
      { cx: 41, cy: 39, r: 5 + TETHER_GAP },
    ]);
  });
});
