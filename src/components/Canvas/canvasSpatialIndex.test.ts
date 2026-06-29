// Run: npx tsx src/components/Canvas/canvasSpatialIndex.test.ts
//
// The raycast index: point-in-box containment, SMALLEST box when nested, fast at scale.

import { buildSpatialIndex, type IndexedNode } from "./canvasSpatialIndex";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const node = (id: string, x: number, y: number, w: number, h: number, kind = "task"): IndexedNode =>
  ({ id, kind, planId: "p", box: { x, y, width: w, height: h } });

// Basic containment.
{
  const idx = buildSpatialIndex([node("a", 0, 0, 100, 100), node("b", 500, 500, 100, 100)]);
  ok("point inside a → a", idx.queryPoint({ x: 50, y: 50 })?.id === "a");
  ok("point inside b → b", idx.queryPoint({ x: 550, y: 550 })?.id === "b");
  ok("point in empty space → null", idx.queryPoint({ x: 300, y: 300 }) === null);
  ok("point on the far edge still contained", idx.queryPoint({ x: 100, y: 100 })?.id === "a");
}

// Nested: a small task box inside a big plan box → the SMALLEST containing wins.
{
  const idx = buildSpatialIndex([
    node("plan", 0, 0, 1000, 1000, "plan"),
    node("task", 400, 400, 80, 80, "task"),
  ]);
  ok("nested point → smallest box (task)", idx.queryPoint({ x: 420, y: 420 })?.id === "task");
  ok("inside big box but outside small → big box", idx.queryPoint({ x: 50, y: 50 })?.id === "plan");
}

// Scale: thousands of small boxes on a grid; every centre resolves to its own box.
{
  const nodes: IndexedNode[] = [];
  for (let i = 0; i < 5000; i++) {
    const gx = (i % 100) * 60;
    const gy = Math.floor(i / 100) * 60;
    nodes.push(node(`n${i}`, gx, gy, 40, 40));
  }
  const idx = buildSpatialIndex(nodes);
  ok("index holds all nodes", idx.size === 5000);
  let hits = 0;
  for (let i = 0; i < 5000; i++) {
    const gx = (i % 100) * 60 + 20;
    const gy = Math.floor(i / 100) * 60 + 20;
    if (idx.queryPoint({ x: gx, y: gy })?.id === `n${i}`) hits++;
  }
  ok(`every centre raycasts to its own box (${hits}/5000)`, hits === 5000);
}

// A huge box (overflow list) is still found.
{
  const idx = buildSpatialIndex([node("huge", -5000, -5000, 20000, 20000, "plan"), node("small", 0, 0, 50, 50)]);
  ok("oversized box found via overflow", idx.queryPoint({ x: 9000, y: 9000 })?.id === "huge");
  ok("small still wins where nested", idx.queryPoint({ x: 25, y: 25 })?.id === "small");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
