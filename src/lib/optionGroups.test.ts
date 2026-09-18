// Run: npx tsx src/lib/optionGroups.test.ts

import { groupOptionsByLabel } from "./optionGroups";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("no option declares a group ⇒ null, so callers keep the flat list", () => {
  assert(
    groupOptionsByLabel([{ id: "a" }, { id: "b" }]) === null,
    "ungrouped options must not fabricate sections",
  );
});

test("options bucket by label, preserving first-appearance order of sections", () => {
  const sections = groupOptionsByLabel([
    { id: "a", group: "alpha" },
    { id: "b", group: "beta" },
    { id: "c", group: "alpha" },
  ]);
  assert(sections !== null, "a declared group produces sections");
  assert(
    sections.map((section) => section.label).join(",") === "alpha,beta",
    `section order must follow first appearance, got ${sections.map((s) => s.label).join(",")}`,
  );
  assert(
    sections[0].options.map((option) => option.id).join(",") === "a,c",
    "catalog order survives inside a section",
  );
});

test("ungrouped options keep their place in a headerless section", () => {
  const sections = groupOptionsByLabel([
    { id: "plain" },
    { id: "a", group: "alpha" },
    { id: "also-plain" },
  ]);
  assert(sections !== null, "one grouped option makes the list grouped");
  assert(sections[0].label === "", "the leading ungrouped bucket has no header");
  assert(
    sections[0].options.map((option) => option.id).join(",") === "plain,also-plain",
    "ungrouped options collect together",
  );
  assert(sections[1].label === "alpha", "the named section follows");
});

test("a whitespace-only group is treated as ungrouped", () => {
  assert(
    groupOptionsByLabel([{ id: "a", group: "   " }]) === null,
    "blank group labels are not sections",
  );
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`optionGroups tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
