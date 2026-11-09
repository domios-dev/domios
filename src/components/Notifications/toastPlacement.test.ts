import { expect, it } from "vitest";
import { toastStackTop } from "./toastPlacement";
it("keeps the stack below visible title and tab controls in its horizontal lane", () => {
  const title = { left: 0, right: 1000, top: 0, bottom: 44, width: 1000, height: 44 };
  const tab = { left: 200, right: 1000, top: 44, bottom: 88, width: 800, height: 44 };
  const otherPane = { ...tab, left: 0, right: 100, bottom: 200 };
  expect(toastStackTop({ left: 600, right: 1000 }, [title, tab, otherPane, { ...tab, height: 0, bottom: 500 }])).toBe(94);
});
