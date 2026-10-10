import { formatDsTasksBlock, parseDsTasksMessage, type DsTasksBlock } from "./dsTasksBlock";

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

const BLOCK: DsTasksBlock = {
  planId: "pl_alpha",
  planTitle: "Alpha\nplan",
  filter: ["in_review", "todo"],
  items: [
    { id: "task_a", key: "AL-1", status: "in_review", name: "First  task" },
    { id: "task_b", key: "AL-2", status: "pending", name: "" },
  ],
};

{
  const text = formatDsTasksBlock(BLOCK);
  const parsed = parseDsTasksMessage(`look at this\n${text}\nthanks`);
  assert(parsed !== null, "a formatted block parses back");
  assert(parsed?.block.planId === "pl_alpha", "plan id round-trips");
  assert(parsed?.block.planTitle === "Alpha plan", "plan title collapses to one line");
  assert(parsed?.block.filter.join(",") === "in_review,todo", "filter round-trips");
  assert(parsed?.block.items.length === 2, "every item round-trips");
  assert(parsed?.block.items[0].id === "task_a" && parsed.block.items[0].key === "AL-1", "item identity round-trips");
  assert(parsed?.block.items[0].status === "in_review", "item status round-trips");
  assert(parsed?.block.items[0].name === "First task", "item name collapses whitespace");
  assert(parsed?.block.items[1].name === "", "an empty name survives");
  assert(parsed?.before === "look at this" && parsed.after === "thanks", "surrounding text is kept apart");
}

{
  const text = formatDsTasksBlock({ ...BLOCK, filter: [] }).replace(/\n/g, "\r\n");
  const parsed = parseDsTasksMessage(text);
  assert(parsed?.block.filter.length === 0, "a block without filter parses with an empty filter");
  assert(parsed?.block.items.length === 2, "CRLF line endings parse");
}

assert(parseDsTasksMessage("plain message about @ds-tasks") === null, "text without a block does not parse");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
