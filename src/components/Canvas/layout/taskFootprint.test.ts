// Run: npx tsx src/components/Canvas/layout/taskFootprint.test.ts
//
// The overflow-regression guard: a task card's FOOTPRINT height must reserve at least
// the NEAR content drawCard renders (header + wrapped title + tags + fields). Footprint
// and render share ONE source — cardContent.FIT via nearContentHeight — so they cannot
// drift; if FIT changes, both move together and titles never spill the box again.

import { FIT, nearContentHeight } from "../canvasRender/cardContent";
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

// 1) MULTI-LINE TITLE: a very long name wraps to 2 lines; the footprint must hold the
// full 2-line near card (the exact bug the user hit — titles spilling below the box).
{
  const longTitle = "Implement the full OAuth refresh-token rotation handler with revocation and replay protection across every provider";
  const task: Task = { ...FIXTURE_TASK, name: longTitle, tags: ["area:nav", "pan-zoom"] };
  const fp = taskFootprint(task, [child], s);
  const need = nearContentHeight({ titleLines: 2, hasTags: true, sections: 0, fields: 4 });
  ok(`2-line-title footprint fits near content (${fp.height} >= ${need})`, fp.height >= need);
}

// 2) CONTENT-BOUND: short 1-line title, tags, 4 fields — content height dominates the
// floors, so footprint height EQUALS the FIT-derived near-content height exactly. This
// is the anti-drift lock: re-hardcoding the formula or nudging any FIT row breaks it.
{
  const task: Task = { ...FIXTURE_TASK, name: "Build API", body: "short", tags: ["area:nav"] };
  const fp = taskFootprint(task, [child], s);
  const need = nearContentHeight({ titleLines: 1, hasTags: true, sections: 0, fields: 4 });
  ok(`content-bound footprint == near content (${fp.height} == ${need})`, fp.height === need);
}

// 3) STRUCTURAL: nearContentHeight is purely FIT — no stray constants hidden inside.
{
  const bare = nearContentHeight({ titleLines: 1, hasTags: false, sections: 0, fields: 0 });
  ok("nearContentHeight is FIT-derived", bare === FIT.headerH + FIT.titleLineH + FIT.titleGap + FIT.padY);
  const tagged = nearContentHeight({ titleLines: 1, hasTags: true, sections: 0, fields: 0 });
  ok("a tag row adds exactly FIT.tagRowH", tagged - bare === FIT.tagRowH);
  const twoLine = nearContentHeight({ titleLines: 2, hasTags: false, sections: 0, fields: 0 });
  ok("an extra title line adds exactly FIT.titleLineH", twoLine - bare === FIT.titleLineH);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
