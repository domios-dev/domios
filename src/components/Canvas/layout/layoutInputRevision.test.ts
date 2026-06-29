// Run: npx tsx src/components/Canvas/layout/layoutInputRevision.test.ts

import { layoutInputRevision, plansForLayout, layoutWorkspace } from "./index";
import { plan, section, task, milestone } from "./fixtures";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("layoutInputRevision changes when task footprint inputs change", () => {
  const sections = [section("s")];
  const base = plan("p", "o", 1, sections, [task("t1", ["s"])]);
  const heavy = plan("p", "o", 1, sections, [
    task("t1", ["s"], { body: "x".repeat(900), milestones: [milestone("m1", "status_update")] }),
  ]);
  assert(layoutInputRevision([base]) !== layoutInputRevision([heavy]), "body + milestones");
});

test("layoutInputRevision changes when spec section tree changes", () => {
  const p = plan("p", "o", 1, [section("a")], [], { primary_spec_id: "spec-1" });
  const specA = new Map([["p", [section("a")]]]);
  const specB = new Map([["p", [section("a"), section("b")]]]);
  assert(
    layoutInputRevision([p], specA) !== layoutInputRevision([p], specB),
    "spec section count",
  );
});

test("plansForLayout overlays fetched spec sections for layout", () => {
  const p = plan("p", "o", 1, [section("plan-only")], [], { primary_spec_id: "spec-1" });
  const specSections = [section("spec-a"), section("spec-b")];
  const merged = plansForLayout([p], new Map([["p", specSections]]));
  assert(merged[0].sections.length === 2, "uses spec sections");
  assert(merged[0].sections[0].id === "spec-a", "spec tree");
});

test("layoutInputRevision ignores task status churn (layout stays stable)", () => {
  const sections = [section("s")];
  const pending = plan("p", "o", 1, sections, [task("t1", ["s"], { status: "pending", body: "small" })]);
  const live = plan("p", "o", 1, sections, [task("t1", ["s"], { status: "in_progress", body: "small" })]);
  assert(layoutInputRevision([pending]) === layoutInputRevision([live]), "status-only diff does not affect layout revision");
});

test("layout bounds grow when tasks are added to the same plan id", () => {
  const sections = [section("s")];
  const small = plan("p", "o", 1, sections, [task("t1", ["s"])]);
  const large = plan("p", "o", 2, sections, [
    task("t1", ["s"]),
    task("t2", ["s"]),
    task("t3", ["s"], { body: "z".repeat(500) }),
    task("t4", ["s"], { milestones: [milestone("m1", "testing"), milestone("m2", "review")] }),
  ]);
  const b0 = layoutWorkspace([small]).bounds;
  const b1 = layoutWorkspace([large]).bounds;
  assert(b1.width > b0.width || b1.height > b0.height, "bounds expand with tasks");
});

function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} layoutInputRevision tests passed`);
}

main();
