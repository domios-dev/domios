// Run: npx tsx packages/chat-ui/src/blocks/blockModels.test.ts

import { parseChatBlock, parseChatBlocks } from "./registry";
import { parseChoiceOptions, choiceOutcome } from "./choiceBlockModel";
import { parseTabRef } from "./agentBlockModel";
import { jobActionPlan, jobScheduleLine, parseJobRef, type JobBlockInfo } from "./jobBlockModel";
import { checklistProgress, checklistStatusLabel, parseChecklistId, type ChecklistItemInfo } from "./checklistBlockModel";
import { decodeTasksBlockKey, parseTasksArgument, tasksBlockKey, tasksBlockSource, tasksSpecFromBlock } from "./tasksBlockModel";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("choice_parses_bar_separated_labels_and_rejects_bad_sets", () => {
  assert(parseChoiceOptions("Yes | No")?.join(",") === "Yes,No", "two labels");
  assert(parseChoiceOptions("Ship it|Hold   the  release | Ask Andrey")?.join(",") === "Ship it,Hold the release,Ask Andrey", "whitespace collapses");
  assert(parseChoiceOptions("A \\| B | C")?.[0] === "A | B", "escaped bar stays literal");
  assert(parseChoiceOptions("Only one") === null, "one label is not a choice");
  assert(parseChoiceOptions("Yes | | No") === null, "blank label refused");
  assert(parseChoiceOptions("Yes | yes") === null, "duplicate label refused");
  assert(parseChoiceOptions(["a", "b", "c", "d", "e", "f", "g", "h", "i"].join("|")) === null, "more than eight refused");
  assert(parseChoiceOptions(`Yes | ${"x".repeat(81)}`) === null, "overlong label refused");
});

test("every_new_block_is_a_registry_entry_and_bad_arguments_stay_text", () => {
  for (const line of ["@ds-choice Yes | No", "@ds-agent e3ec9ee9", "@ds-job nightly-digest", "@ds-checklist getting-started", "@ds-tasks pl_a0rcq7 in_progress AL-3"]) {
    assert(parseChatBlock(line), `${line} should parse`);
  }
  for (const line of ["@ds-choice Yes", "@ds-agent a b", "@ds-job ../etc", "@ds-checklist Getting Started", "@ds-tasks plan_x", "@ds-tasks pl_x hello"]) {
    assert(parseChatBlock(line) === null, `${line} should stay text`);
  }
  assert(parseChatBlocks("@ds-agent e3ec9ee9\n@ds-job nightly")?.length === 2, "stacked lines render as stacked blocks");
});

test("refs_validate_their_shape", () => {
  assert(parseTabRef("e3ec9ee9") === "e3ec9ee9" && parseTabRef("abc") === null && parseTabRef("a/b/c/d") === null, "tab ref");
  assert(parseJobRef("cron_01J") === "cron_01J" && parseJobRef("-x") === null, "job ref");
  assert(parseChecklistId("getting-started") === "getting-started" && parseChecklistId("x y") === null, "checklist id");
});

test("choice_outcome_follows_the_transcript", () => {
  const options = ["Ship it", "Hold"];
  assert(choiceOutcome(options, null, null).kind === "unavailable", "no host means unavailable");
  assert(choiceOutcome(options, { reply: null }, null).kind === "open", "no reply is open");
  assert(choiceOutcome(options, { reply: null }, "Hold").kind === "sending", "in flight");
  const chosen = choiceOutcome(options, { reply: "  ship IT " }, null);
  assert(chosen.kind === "chosen" && chosen.label === "Ship it", "matching reply is the choice");
  assert(choiceOutcome(options, { reply: "actually wait" }, null).kind === "answered", "other reply closes the question");
});

const JOB: JobBlockInfo = {
  id: "c1", kind: "cron", name: "Digest", state: "enabled", stateLabel: "Enabled", schedule: "Every day at 09:00",
  nextRun: "in 3h", lastResult: null, canRunNow: true, canPause: true, canStop: true,
};

test("job_actions_offer_only_what_the_state_allows", () => {
  assert(JSON.stringify(jobActionPlan(JOB)) === JSON.stringify({ runNow: true, pause: "pause", stop: false }), "enabled cron");
  assert(jobActionPlan({ ...JOB, state: "paused" }).pause === "resume", "paused cron resumes");
  const running = jobActionPlan({ ...JOB, state: "running" });
  assert(!running.runNow && running.stop, "running cron stops instead of running again");
  const blueprint = jobActionPlan({ ...JOB, kind: "blueprint", canRunNow: false, canPause: false, state: "running" });
  assert(!blueprint.runNow && blueprint.pause === null && blueprint.stop, "blueprint has no fake pause");
  assert(jobScheduleLine(JOB) === "Every day at 09:00, next in 3h" && jobScheduleLine({ ...JOB, state: "paused" }).endsWith("paused"), "schedule line");
});

test("checklist_progress_counts_done_and_unknown", () => {
  const item = (id: string, done: boolean | null): ChecklistItemInfo => ({ id, label: id, hint: "", done, action: { kind: "link", label: "Go" } });
  const progress = checklistProgress([item("a", true), item("b", false), item("c", null)]);
  assert(progress.done === 1 && progress.total === 3 && progress.unknown === 1, "counts");
  assert(checklistStatusLabel(progress) === "1 of 3 done", "label");
  assert(checklistStatusLabel(checklistProgress([item("a", true)])) === "All done", "complete label");
});

test("tasks_argument_round_trips_through_source_and_key", () => {
  const spec = parseTasksArgument("pl_alpha AL-1 task_b done in-review");
  assert(spec && spec.refs.join(",") === "AL-1,task_b" && spec.groups.join(",") === "done,in_review", "refs and groups");
  assert(tasksBlockSource(spec) === "@ds-tasks pl_alpha AL-1 task_b done in_review", "one-line source");
  assert(JSON.stringify(decodeTasksBlockKey(tasksBlockKey(spec))) === JSON.stringify(spec), "key round-trips");
  const carried = tasksSpecFromBlock({ planId: "pl_alpha", planTitle: "Alpha", filter: ["pending"], items: [{ id: "task_a", key: "AL-1", status: "done", name: "A" }] });
  assert(carried.groups[0] === "todo" && carried.items?.length === 1, "carried block keeps its items");
  assert(tasksBlockSource(carried) === "@ds-tasks pl_alpha task_a", "carried set pins as task ids");
  assert(decodeTasksBlockKey("not json") === null, "bad key");
});

let failed = 0;
for (const [name, run] of tests) {
  try {
    run();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
console.log(`chat-ui/blocks/blockModels tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
