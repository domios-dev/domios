import { describe, expect, it } from "vitest";
import { computeFitTransform, touchTransform } from "@codeterm/canvas-camera";
import { DESKTOP_ZOOM_STEP, TOUCH_ZOOM_STEP, inventoryView, listCardScale, meshZoomStep, offersInventoryList } from "./meshViewportModel";
import { webGraphInitialView, webGraphLayout, MACHINE_CARD_WIDTH } from "./webGraphLayout";

describe("mesh inventory on a phone", () => {
  it("a touch phone starts on the list and can switch to the graph; desktop keeps the graph at any width", () => {
    expect(offersInventoryList(390, true)).toBe(true);
    expect(inventoryView(390, true, null)).toBe("list");
    expect(inventoryView(390, true, "graph")).toBe("graph");
    expect(inventoryView(390, false, null)).toBe("graph");
    expect(inventoryView(1280, true, null)).toBe("graph");
    expect(inventoryView(1280, true, "list")).toBe("graph");
    expect(offersInventoryList(0, true)).toBe(false);
  });

  it("list cards keep full size where they fit and shrink only on narrower phones", () => {
    expect(listCardScale(390)).toBe(1);
    expect(listCardScale(375)).toBeCloseTo((375 - 24) / MACHINE_CARD_WIDTH);
    expect(listCardScale(1200)).toBe(1);
  });

  it("+/- step is visible on touch and unchanged on desktop", () => {
    expect(meshZoomStep(true)).toBe(TOUCH_ZOOM_STEP);
    expect(meshZoomStep(false)).toBe(DESKTOP_ZOOM_STEP);
    expect(DESKTOP_ZOOM_STEP).toBe(0.06);
  });
});

describe("mesh graph camera", () => {
  it("fits seven machines into a 390x700 phone centred, which is why the list is the readable default", () => {
    const geometry = webGraphLayout(390, 700, 7, 300);
    const xs = geometry.machines.map((machine) => machine.x);
    const ys = geometry.machines.map((machine) => machine.y);
    const bounds = {
      x: Math.min(...xs) - MACHINE_CARD_WIDTH / 2, y: Math.min(...ys) - 130,
      width: Math.max(...xs) - Math.min(...xs) + MACHINE_CARD_WIDTH, height: Math.max(...ys) - Math.min(...ys) + 260,
    };
    const initial = webGraphInitialView(390, 700, bounds)!;
    const fit = computeFitTransform(initial.bounds, 390, 700, { fitPadding: 24, maxScale: 1 })!;
    expect(fit.scale).toBeLessThan(0.35);
    expect(fit.x + (bounds.x + bounds.width / 2) * fit.scale).toBeCloseTo(195);
    expect(fit.y + (bounds.y + bounds.height / 2) * fit.scale).toBeCloseTo(350);
  });

  it("pinching apart doubles the scale around the fingers, and one finger pans", () => {
    const base = { x: 0, y: 0, scale: 0.5 };
    const pinched = touchTransform(base, [{ x: 100, y: 100 }, { x: 200, y: 100 }], [{ x: 50, y: 100 }, { x: 250, y: 100 }], 0.1, 2.5);
    expect(pinched.scale).toBe(1);
    expect(pinched.x + 300 * pinched.scale).toBeCloseTo(150);
    const panned = touchTransform(base, [{ x: 100, y: 100 }], [{ x: 130, y: 80 }], 0.1, 2.5);
    expect(panned).toEqual({ x: 30, y: -20, scale: 0.5 });
    expect(touchTransform(base, [{ x: 0, y: 0 }, { x: 10, y: 0 }], [{ x: 0, y: 0 }, { x: 1000, y: 0 }], 0.1, 2.5).scale).toBe(2.5);
  });
});
