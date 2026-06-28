// Run: npx tsx src/components/Canvas/canvasView/useCanvasNavHistory.test.ts

import { pushEntry, truncatePath, type NavEntry } from "./useCanvasNavHistory";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const e = (id: string, kind: NavEntry["kind"] = "task"): NavEntry => ({ id, kind, label: id, flyIds: [id] });

// ── push: appends, dedups the current top (re-fly same node = no new hop) ──
{
  let p: NavEntry[] = [];
  p = pushEntry(p, e("sec"));
  p = pushEntry(p, e("task"));
  ok("push grows the path", p.length === 2 && p[0].id === "sec" && p[1].id === "task");
  const same = pushEntry(p, e("task"));
  ok("re-pushing the current top is a no-op (same ref)", same === p);
  const back = pushEntry(p, e("sec"));
  ok("pushing a non-top id appends (no global dedup, real path)", back.length === 3);
}

// ── back: pop one hop ──
{
  const p = [e("sec"), e("task"), e("spec")];
  ok("back pops the last hop", p.slice(0, -1).length === 2);
}

// ── truncate: crumb click flies to that hop, drops everything after ──
{
  const p = [e("sec"), e("task"), e("spec")];
  ok("truncate(0) keeps only the first hop", truncatePath(p, 0).length === 1 && truncatePath(p, 0)[0].id === "sec");
  ok("truncate(1) keeps two hops", truncatePath(p, 1).length === 2);
  ok("truncate(-1) clears to home (plan)", truncatePath(p, -1).length === 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
