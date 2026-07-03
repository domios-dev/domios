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
  assert(ids.length === 8, `expected 8 sections, got ${ids.length}`);
  assert(ids.includes("plans"), "missing plans section");
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
