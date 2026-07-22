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
const PLAN2 = "pl_" + "e".repeat(32);
const TASK3 = "task_" + "f".repeat(32);

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

// Safe boundaries: commands, URLs, and fenced/inline code stay plain text.
{
  assert(!hasPlanRefs(`codeterm plan show ${PLAN}`), "command line not linked");
  assert(!hasPlanRefs(`git show ${PLAN}`), "git command not linked");
  assert(!hasPlanRefs(`See https://example.test/${PLAN}`), "url not linked");
  assert(!hasPlanRefs("```txt\npl_hidden task_hidden\n```"), "fenced code not linked");
  assert(!hasPlanRefs(`before\n\`task_hidden ${PLAN}\`\nafter`), "inline code not linked");
  assert(!hasPlanRefs(`before\n\`\`\`txt\n${PLAN}\ntask_hidden`), "unterminated fenced code not linked");
  const prose = `Investigate ${PLAN} then review ${TASK}`;
  assert(extractPlanRefs(prose).length === 2, "prose refs still link");
}

// Multiple plan/task pairs must stay locally paired.
{
  const text = `${PLAN} owns ${TASK}\n${PLAN2} owns ${TASK3}`;
  const refs = extractPlanRefs(text);
  assert(refs.length === 4, "two local plan/task pairs link");
  assert(refs.some((s) => s.targetKind === "task" && s.targetId === TASK && s.planId === PLAN), "first task stays on first plan");
  assert(refs.some((s) => s.targetKind === "task" && s.targetId === TASK3 && s.planId === PLAN2), "second task stays on second plan");
}

// A task on a later line must not inherit an earlier plan id.
{
  const segs = linkifyPlanRefs(`${PLAN}\n${TASK}`);
  assert(segs.filter((s) => s.kind === "plan_ref").length === 1, "task on later line stays plain text");
}

// Unpaired task before a plan stays plain text; later paired task still links.
{
  const text = `${TASK} then ${PLAN} and ${TASK2}`;
  const refs = extractPlanRefs(text);
  assert(refs.length === 2, "only plan and paired trailing task link");
  assert(refs.some((s) => s.targetKind === "task" && s.targetId === TASK2 && s.planId === PLAN), "paired trailing task links");
  assert(!refs.some((s) => s.targetKind === "task" && s.targetId === TASK), "leading unpaired task stays plain");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
