import { expect, it } from "vitest";
import { parseNeighbors, validNeighborLimit } from "./neighborModel";

it("accepts the server's locked registry so Free can manage existing connections", () => {
  const list = { schema_version: 1, access: "locked", friends: [], incoming: [], outgoing: [] };
  expect(parseNeighbors(list)).toEqual(list);
  expect(() => parseNeighbors({ ...list, access: "free" })).toThrow();
  expect(() => parseNeighbors({ ...list, incoming: [null] })).toThrow();
});

it("preserves bounded own outgoing rows without inventing recipient profiles", () => {
  const outgoing = [{ id: "a".repeat(64), typed_email: "recipient@example.test", created_at: 42, state: "pending" }];
  const list = { schema_version: 1, access: "paid", friends: [], incoming: [], outgoing };
  expect(parseNeighbors(list).outgoing).toEqual(outgoing);
  for (const invalid of [undefined, [null], [{ ...outgoing[0], id: "bad" }], [{ ...outgoing[0], state: "accepted" }], Array(201).fill(outgoing[0])]) {
    expect(() => parseNeighbors({ ...list, outgoing: invalid })).toThrow();
  }
});

it("permits only integer inbound limits from one through two hundred", () => {
  for (const value of [1, 42, 200]) expect(validNeighborLimit(value)).toBe(true);
  for (const value of [0, -1, 201, 1.5, NaN]) expect(validNeighborLimit(value)).toBe(false);
});
