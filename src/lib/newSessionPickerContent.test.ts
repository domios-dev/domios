// Run: npx tsx src/lib/newSessionPickerContent.test.ts

import { buildDestinationPreview } from "./newSessionPickerContent";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assertEqual(actual: unknown, expected: unknown, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`);
  }
}

test("pair mode always mentions Orchestrator + Planner and the destination workspace", () => {
  const line = buildDestinationPreview({ mode: "pair", workspaceName: "Acme" });
  assertEqual(line, "Orchestrator + Planner will open as one group in Acme", "pair preview text");
});

test("single mode with a model shows provider (model) in workspace", () => {
  const line = buildDestinationPreview({
    mode: "single",
    workspaceName: "Acme",
    singleProviderLabel: "Claude Code",
    singleModelLabel: "Opus 4.8",
  });
  assertEqual(line, "Claude Code (Opus 4.8) will open in Acme", "single preview with model");
});

test("single mode without a model omits the parens", () => {
  const line = buildDestinationPreview({
    mode: "single",
    workspaceName: "Acme",
    singleProviderLabel: "Claude Code",
  });
  assertEqual(line, "Claude Code will open in Acme", "single preview without model");
});

test("single mode with neither provider nor model falls back to a generic label", () => {
  const line = buildDestinationPreview({ mode: "single", workspaceName: "Acme" });
  assertEqual(line, "Agent will open in Acme", "single preview fallback label");
});

test("preview line updates when the destination workspace changes", () => {
  const a = buildDestinationPreview({ mode: "pair", workspaceName: "Acme" });
  const b = buildDestinationPreview({ mode: "pair", workspaceName: "Bravo" });
  if (a === b) throw new Error("preview must change when workspace changes");
  assertEqual(b, "Orchestrator + Planner will open as one group in Bravo", "updated workspace preview");
});

function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`newSessionPickerContent tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

main();
