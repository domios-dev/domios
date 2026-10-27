import { expect, it } from "vitest";
import { parseNeighbors, validNeighborLimit } from "./neighborModel";

it("accepts the server's locked registry so Free can manage existing connections", () => {
  const list = { schema_version: 1, access: "locked", friends: [], incoming: [] };
  expect(parseNeighbors(list)).toEqual(list);
  expect(() => parseNeighbors({ ...list, access: "free" })).toThrow();
  expect(() => parseNeighbors({ ...list, incoming: [null] })).toThrow();
});

it("permits only integer inbound limits from one through two hundred", () => {
  for (const value of [1, 42, 200]) expect(validNeighborLimit(value)).toBe(true);
  for (const value of [0, -1, 201, 1.5, NaN]) expect(validNeighborLimit(value)).toBe(false);
});
