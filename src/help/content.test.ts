// Run: npx tsx src/help/content.test.ts

import { getHelpContent, getHelpSectionIds } from "./content";
import type { HelpBlock } from "./types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("english content exposes all guide sections", () => {
  const ids = getHelpSectionIds("en");
  assert(ids.length === 9, `expected 9 sections, got ${ids.length}`);
  assert(ids.includes("plans"), "missing plans section");
  assert(ids.includes("decision-rerank"), "missing decision re-rank section");
  assert(ids.includes("tips"), "missing tips section");
});

test("content blocks are structured data without JSX copy in renderer paths", () => {
  const { sections } = getHelpContent("en");
  for (const section of sections) {
    assert(section.title.length > 0, `${section.id} missing title`);
    assert(section.subtitle.length > 0, `${section.id} missing subtitle`);
    assert(section.blocks.length > 0, `${section.id} has no blocks`);
    for (const block of section.blocks as HelpBlock[]) {
      if (block.type === "paragraph") assert(block.text.length > 10, "paragraph too short");
      if (block.type === "figure") assert(!!block.figureId, "figure missing id");
      if (block.type === "shortcut-table") assert(block.rows.length > 0, "empty shortcut table");
    }
  }
});

test("every section offers an ask-the-agent button with a substantial question", () => {
  const { sections } = getHelpContent("en");
  for (const section of sections) {
    const asks = section.blocks.filter((b) => b.type === "ask");
    assert(asks.length === 1, `${section.id} must have exactly one ask block, got ${asks.length}`);
    const ask = asks[0];
    if (ask.type === "ask") {
      assert(ask.label.length > 10, `${section.id} ask label too short`);
      assert(ask.question.length > 60, `${section.id} ask question too short to steer the agent`);
    }
  }
});

test("agents section explains single-authority planner attachment", () => {
  const agents = getHelpContent("en").sections.find((s) => s.id === "agents");
  assert(!!agents, "agents section missing");
  const text = agents!.blocks
    .map((b) => (b.type === "paragraph" || b.type === "tip" ? b.text : b.type === "list" ? b.items.join(" ") : ""))
    .join(" ");
  assert(text.includes("Planner") && text.includes("Orchestrator"), "pair not named");
  assert(/single authority/i.test(text), "single-authority framing missing");
  assert(/subordinate planning specialist/i.test(text), "planner-specialist framing missing");
  assert(!/equals|same level|Neither is the boss/i.test(text), "old peer-equality framing leaked");
});

test("plans section includes pipeline and status figures", () => {
  const plans = getHelpContent("en").sections.find((s) => s.id === "plans");
  assert(!!plans, "plans section missing");
  const figures = plans!.blocks.filter((b) => b.type === "figure").map((b) => (b.type === "figure" ? b.figureId : ""));
  assert(figures.includes("plan-pipeline"), "missing plan-pipeline figure");
  assert(figures.includes("task-status-flow"), "missing task-status-flow figure");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`help/content tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
