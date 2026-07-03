// Run: npx tsx src/components/Settings/sections.test.ts

import { SETTINGS_SECTIONS, sectionTitle, type SectionId } from "./sections";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("guide section sits between logs and about", () => {
  const ids = SETTINGS_SECTIONS.map((s) => s.id);
  const logsIdx = ids.indexOf("logs");
  const guideIdx = ids.indexOf("guide");
  const aboutIdx = ids.indexOf("about");
  assert(logsIdx >= 0 && guideIdx >= 0 && aboutIdx >= 0, "missing nav ids");
  assert(guideIdx === logsIdx + 1, "guide should follow logs");
  assert(aboutIdx === guideIdx + 1, "about should follow guide");
});

test("guide section title resolves", () => {
  assert(sectionTitle("guide" as SectionId) === "Guide", "guide title");
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
console.log(`Settings/sections tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
