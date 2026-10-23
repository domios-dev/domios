import { expect, it } from "vitest";
import { sidebarDropEdge, sidebarDropTarget } from "./sidebarDropGeometry";

const rows = [
  { id: "owner", top: 0, bottom: 100 },
  { id: "short", top: 100, bottom: 130 },
  { id: "browser-owner", top: 130, bottom: 250 },
  { id: "last", top: 250, bottom: 280 },
];

it("uses remaining row midpoints without moving unequal-height browser subtrees", () => {
  expect(sidebarDropTarget(rows, "short", 49)).toBe("owner");
  expect(sidebarDropTarget(rows, "short", 50)).toBe("short");
  expect(sidebarDropTarget(rows, "short", 189)).toBe("short");
  expect(sidebarDropTarget(rows, "short", 190)).toBe("browser-owner");
  expect(sidebarDropTarget(rows, "short", 265)).toBe("last");
  expect(sidebarDropTarget(rows, "browser-owner", 0)).toBe("owner");
  expect(sidebarDropTarget(rows, "owner", 300)).toBe("last");
});

it("places the marker on the target boundary matching the committed move", () => {
  const ids = rows.map(row => row.id);
  expect(sidebarDropEdge(ids, "short", "owner")).toBe("top");
  expect(sidebarDropEdge(ids, "short", "browser-owner")).toBe("bottom");
  expect(sidebarDropEdge(ids, "short", "short")).toBeNull();
  expect(sidebarDropEdge(ids, "missing", "short")).toBeNull();
  expect(sidebarDropTarget(rows, "missing", 100)).toBeNull();
});
