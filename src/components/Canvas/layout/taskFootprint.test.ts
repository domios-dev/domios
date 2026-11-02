
import { nearContentHeight } from "../canvasRender/cardContent";
import { DEFAULT_CANVAS_SETTINGS } from "../settings";
import type { Task } from "../types";
import { FIXTURE_TASK } from "../cards/fixtures.testdata";
import { taskFootprint } from "./taskFootprint";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const s = DEFAULT_CANVAS_SETTINGS;
const child: Task = { ...FIXTURE_TASK, id: "sub-1", name: "subtask", body: "x", parent_task_id: FIXTURE_TASK.id };

// full 2-line near card (the exact bug the user hit — titles spilling below the box).
{
  const longTitle = "Implement the full OAuth refresh-token rotation handler with revocation and replay protection across every provider";
  const task: Task = { ...FIXTURE_TASK, name: longTitle, tags: ["area:nav", "pan-zoom"] };
  const fp = taskFootprint(task, [child], s);
  const need = nearContentHeight({ titleLines: 2, hasTags: true, sections: 0, fields: 4 });
  ok(`2-line-title footprint fits near content (${fp.height} >= ${need})`, fp.height >= need);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
