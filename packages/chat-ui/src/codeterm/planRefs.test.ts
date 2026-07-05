import { linkifyPlanRefs, hasPlanRefs } from "./planRefs";

let passed = 0;
let failed = 0;
function assert(cond: unknown, name: string) {
  if (cond) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}`);
  }
}

const PLAN = "pl_" + "a".repeat(32);
const TASK = "task_" + "b".repeat(32);
const TASK2 = "task_" + "c".repeat(32);

// No plan context → never link.
{
  const segs = linkifyPlanRefs(`working on ${TASK} now`);
  assert(segs.length === 1 && segs[0].kind === "text", "no link without plan context");
  assert(!hasPlanRefs(`just ${TASK}`), "hasPlanRefs false without plan id");
}

// With plan context → task ids become links.
{
  const text = `plan ${PLAN}: done ${TASK} and ${TASK2}`;
  const segs = linkifyPlanRefs(text);
  const tasks = segs.filter((s) => s.kind === "task");
  assert(tasks.length === 2, "both task ids linked");
  assert(tasks[0].kind === "task" && (tasks[0] as any).taskId === TASK, "first task id captured");
  assert(tasks.every((s) => s.kind === "task" && (s as any).planId === PLAN), "links carry plan id");
  assert(hasPlanRefs(text), "hasPlanRefs true with context");
  // round-trips the text
  assert(segs.map((s) => s.value).join("") === text, "segments reconstruct original text");
}

// Conservative: arbitrary hex is not a false link.
{
  const segs = linkifyPlanRefs(`sha ${"d".repeat(40)} in ${PLAN}`);
  assert(segs.every((s) => s.kind === "text"), "raw hex sha never linked");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
