import assert from "node:assert/strict";
import test from "node:test";
import { pickHint, splitHint, type HintTarget } from "./hints";

const target = (hint: string, left: number, top: number, width: number, height: number): HintTarget => ({ hint, rect: { left, top, width, height } });
const graph = target("graph", 0, 0, 1000, 600);
const machine = target("machine", 100, 100, 200, 120);

test("the innermost hinted element under the pointer wins", () => {
  assert.equal(pickHint([graph, machine], 150, 150)?.hint, "machine");
  assert.equal(pickHint([machine, graph], 150, 150)?.hint, "machine");
  assert.equal(pickHint([graph, machine], 500, 400)?.hint, "graph");
});

test("nothing is picked outside every box or over a collapsed one", () => {
  assert.equal(pickHint([machine], 50, 50), null);
  assert.equal(pickHint([target("gone", 0, 0, 0, 0)], 0, 0), null);
});

test("a hint splits into its title and description at the first colon", () => {
  assert.deepEqual(splitHint("Blueprint: agents: wired"), ["Blueprint", "agents: wired"]);
  assert.deepEqual(splitHint("Plugins"), ["Plugins", ""]);
});
