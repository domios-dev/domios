import { extractPlanRefs, linkifyPlanRefs, hasPlanRefs } from "./planRefs";

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
const SHORT_PLAN = "pl_qmfevk";
const SHORT_TASK = "task_a5es5z";
const SECTION = "sec_vecy77";

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
  const tasks = segs.filter((s) => s.kind === "plan_ref" && s.targetKind === "task");
  assert(tasks.length === 2, "both task ids linked");
  assert(tasks[0].kind === "plan_ref" && (tasks[0] as any).targetId === TASK, "first task id captured");
  assert(tasks.every((s) => s.kind === "plan_ref" && (s as any).planId === PLAN), "links carry plan id");
  assert(hasPlanRefs(text), "hasPlanRefs true with context");
  // round-trips the text
  assert(segs.map((s) => s.value).join("") === text, "segments reconstruct original text");
}

// Short CodeTerm ids: plan links stand alone; task/section links use plan context.
{
  const text = `open ${SHORT_PLAN}, then ${SECTION} and ${SHORT_TASK}`;
  const refs = extractPlanRefs(text);
  assert(refs.length === 3, "short plan, section, and task ids link");
  assert(refs.some((s) => s.targetKind === "plan" && s.planId === SHORT_PLAN), "short plan ref captured");
  assert(refs.some((s) => s.targetKind === "section" && s.targetId === SECTION && s.planId === SHORT_PLAN), "section carries plan id");
  assert(refs.some((s) => s.targetKind === "task" && s.targetId === SHORT_TASK && s.planId === SHORT_PLAN), "short task carries plan id");
}

// Conservative: arbitrary hex is not a false link.
{
  const segs = linkifyPlanRefs(`sha ${"d".repeat(40)} in ${PLAN}`);
  const refs = segs.filter((s) => s.kind === "plan_ref");
  assert(refs.length === 1 && refs[0].kind === "plan_ref" && refs[0].targetKind === "plan", "raw hex sha never linked");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
