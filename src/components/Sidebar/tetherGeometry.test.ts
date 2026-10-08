import { describe, expect, it } from "vitest";
import { TETHER_GAP, TETHER_RADIUS, tetherPaths, type TetherRect } from "./tetherGeometry";

const owner: TetherRect = { left: 16, top: 8, width: 12, height: 12 };

describe("tether geometry", () => {
  it("runs from under the owner's icon, rounds the corner and stops short of the attachment icon", () => {
    const [d] = tetherPaths(owner, [{ left: 36, top: 34, width: 10, height: 10 }]);
    const x0 = 22;
    const y0 = 20 + TETHER_GAP;
    const y1 = 39;
    expect(d).toBe(`M${x0} ${y0}V${y1 - TETHER_RADIUS}Q${x0} ${y1} ${x0 + TETHER_RADIUS} ${y1}H${36 - TETHER_GAP}`);
  });

  it("draws one strand per attachment, each a single hop from the owner", () => {
    const paths = tetherPaths(owner, [
      { left: 36, top: 34, width: 10, height: 10 },
      { left: 36, top: 54, width: 10, height: 10 },
    ]);
    expect(paths).toHaveLength(2);
    for (const d of paths) expect(d.startsWith("M22 22V")).toBe(true);
    expect(paths[1]).toContain("H34");
  });

  it("tightens the corner radius when the attachment sits close to the trunk", () => {
    const [d] = tetherPaths(owner, [{ left: 27, top: 24, width: 6, height: 6 }]);
    expect(d).toBe("M22 22V24Q22 27 25 27H25");
  });

  it("falls back to a straight drop when the attachment icon is directly under the owner", () => {
    const [d] = tetherPaths(owner, [{ left: 17, top: 34, width: 10, height: 10 }]);
    expect(d).toBe(`M22 22V${34 - TETHER_GAP}`);
  });

  it("never draws upward or from unmeasured boxes", () => {
    expect(tetherPaths(owner, [{ left: 36, top: 0, width: 10, height: 10 }])).toEqual([""]);
    expect(tetherPaths(owner, [{ left: 0, top: 0, width: 0, height: 0 }])).toEqual([""]);
    expect(tetherPaths({ left: 0, top: 0, width: 0, height: 0 }, [{ left: 36, top: 34, width: 10, height: 10 }])).toEqual([""]);
  });

  it("rounds coordinates so sub-pixel layout does not churn the path", () => {
    const [d] = tetherPaths({ left: 16.333, top: 8.111, width: 12, height: 12 }, [{ left: 36.6666, top: 34, width: 10, height: 10 }]);
    expect(d).toBe("M22.33 22.11V33Q22.33 39 28.33 39H34.67");
  });
});
