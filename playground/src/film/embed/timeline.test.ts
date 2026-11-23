import assert from "node:assert/strict";
import test from "node:test";
import type { Cut } from "../reels/library";
import { chapterAt, chaptersOf, cuesAt, isWeakDevice, reachAt, loopFade, loopWindow, MEMORY_GRAPH_CUTS, spanOf } from "./timeline";

const cut = (id: string, start: number, end: number): Cut => ({ id, start, end, from: 0 });
const cuts = [
  cut("lab-memory-hook", 0, 8),
  cut("lab-memory-origin", 8, 19),
  cut("memory-graph", 19, 27),
  cut("lab-dreaming", 27, 43),
  cut("mesh-hero", 43, 48),
  cut("team-return", 48, 60),
  cut("blueprint", 60, 74),
  cut("chat-automation", 74, 79),
  cut("browser", 79, 82),
  cut("full-interface", 82, 92),
  cut("end-card", 92, 96),
];

test("chapters open at their first present cut and the first chapter opens the reel", () => {
  const chapters = chaptersOf(cuts, 96);
  assert.deepEqual(chapters.map((c) => [c.title, c.start, c.end]), [
    ["Memory", 0, 43],
    ["Mesh", 43, 48],
    ["Team", 48, 60],
    ["Blueprints", 60, 74],
    ["Jobs & chat", 74, 79],
    ["Browser & plugins", 79, 82],
    ["Interface", 82, 96],
  ]);
  assert.equal(chapterAt(chapters, 47.9)?.title, "Mesh");
  assert.equal(chapterAt(chapters, 95)?.title, "Interface");
});

test("the memory-graph span is the contiguous run starting at the first graph cut", () => {
  assert.deepEqual(spanOf(cuts, MEMORY_GRAPH_CUTS), { start: 0, end: 43 });
  assert.deepEqual(spanOf(cuts.slice(1), MEMORY_GRAPH_CUTS), { start: 8, end: 43 });
  assert.equal(spanOf(cuts.slice(4), MEMORY_GRAPH_CUTS), null);
});


test("a device is weak only once enough frames show a median above the budget", () => {
  assert.equal(isWeakDevice(Array(29).fill(40)), null);
  assert.equal(isWeakDevice([...Array(15).fill(16), ...Array(15).fill(40)]), true);
  assert.equal(isWeakDevice([...Array(16).fill(16), ...Array(14).fill(40)]), false);
  assert.equal(isWeakDevice(Array(30).fill(24)), false);
});

test("a scene card loops its window inside the cut, clamped to the cut", () => {
  const graph = cut("memory-graph", 19, 27);
  assert.deepEqual(loopWindow(graph), { from: 19, to: 27 });
  assert.deepEqual(loopWindow(graph, [1, 6.5]), { from: 20, to: 25.5 });
  assert.deepEqual(loopWindow(graph, [-2, 40]), { from: 19, to: 27 });
  assert.equal(loopWindow(undefined), null);
  assert.equal(loopWindow(graph, [5, 5]), null);
});

test("a loop fades in from its start and out before it wraps", () => {
  assert.equal(loopFade(0, 8), 0);
  assert.equal(loopFade(4, 8), 1);
  assert.equal(loopFade(0.2, 8), 0.5);
  assert.ok(Math.abs(loopFade(7.8, 8) - 0.5) < 1e-9);
});

test("cues are active over their share of their cut, several at once", () => {
  const cues = [{ cue: "jev", cut: "blueprint", from: 0.5, to: 1 }, { cue: "night", cut: "blueprint", from: 0.25, to: 0.75 }];
  assert.deepEqual(cuesAt(cuts, 63, cues), []);
  assert.deepEqual(cuesAt(cuts, 65, cues), ["night"]);
  assert.deepEqual(cuesAt(cuts, 68, cues), ["jev", "night"]);
  assert.deepEqual(cuesAt(cuts, 74, cues), []);
  assert.deepEqual(cuesAt(cuts, 68, [{ cue: "x", cut: "missing", from: 0, to: 1 }]), []);
});

test("the spill reaches out over the scenic span and draws back before it ends", () => {
  const cues = [{ cue: "scenic", cut: "lab-memory-hook", from: 0, to: 1 }, { cue: "scenic", cut: "memory-graph", from: 0, to: 0.5 }];
  assert.equal(reachAt(cuts, 0, cues), 0);
  assert.equal(reachAt(cuts, 10, cues), 1);
  assert.ok(reachAt(cuts, 0.8, cues) > 0 && reachAt(cuts, 0.8, cues) < 1);
  assert.ok(reachAt(cuts, 22, cues) > 0 && reachAt(cuts, 22, cues) < 1);
  assert.equal(reachAt(cuts, 23, cues), 0);
  assert.equal(reachAt(cuts, 10, []), 0);
});
