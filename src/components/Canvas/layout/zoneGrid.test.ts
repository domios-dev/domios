// Run: npx tsx src/components/Canvas/layout/zoneGrid.test.ts

import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import { layoutZoneGrid } from "./zoneGrid";
import { plan, section, task } from "./fixtures";
import { boxesOverlap } from "./geometry";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

{
  const sections = [section("s0"), section("s1")];
  const tasks = [];
  for (let s = 0; s < 2; s++)
    for (let i = 0; i < 8; i++) tasks.push(task(`s${s}_t${i}`, [`s${s}`], { created_at: s * 10 + i }));
  const grid = layoutZoneGrid(plan("p1", "o", 0, sections, tasks), S, new Map());
  ok("two zone containers", grid.zoneBoxes.size === 2);
  ok("16 task nodes", grid.nodes.length === 16);

  const taskNodes = grid.nodes;
  const uniform = taskNodes.every((n) => n.box.width === S.zoneTaskW && n.box.height === S.zoneTaskH);
  ok("uniform task cell size", uniform);

  let overlap = false;
  for (let i = 0; i < taskNodes.length; i++)
    for (let j = i + 1; j < taskNodes.length; j++)
      if (boxesOverlap(taskNodes[i].box, taskNodes[j].box)) overlap = true;
  ok("no task overlap in grid", !overlap);

  for (const [sid, box] of grid.zoneBoxes) {
    const members = taskNodes.filter((n) => {
      const t = tasks.find((tt) => tt.id === n.id);
      return (t?.section_ids[0] ?? "__unsectioned__") === sid;
    });
    const inside = members.every((n) => n.box.x >= box.x && n.box.y >= box.y && n.box.x + n.box.width <= box.x + box.width + 1 && n.box.y + n.box.height <= box.y + box.height + 1);
    ok(`tasks inside zone ${sid}`, inside);
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
