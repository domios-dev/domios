// Run: npx tsx src/components/Canvas/planView/planRetrospective.test.ts

import { FIXTURE_MILESTONE, FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Milestone, Plan } from "../types";
import { latestRetrospective, showRetrospectiveBlock, skillTimelineForSlug } from "./planRetrospective";
import { RETROSPECTIVE_SUPERSEDE_NOTE } from "./planRetrospectiveConstants";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function stride(kind: string, text: string, ts: number): Milestone {
  return { ...FIXTURE_MILESTONE, id: `ms_${ts}`, kind, text, ts };
}

test("latestRetrospective ignores supersede markers", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    strides: [
      stride("retrospective", "first", 100),
      stride("retrospective", RETROSPECTIVE_SUPERSEDE_NOTE, 200),
      stride("retrospective", "final", 300),
    ],
  };
  assert(latestRetrospective(plan)?.text === "final", "picks newest substantive retrospective");
});

test("showRetrospectiveBlock for archived plans", () => {
  const plan: Plan = { ...FIXTURE_PLAN, status: "archived", strides: [] };
  assert(showRetrospectiveBlock(plan), "archived plans show retrospective section");
});

test("skillTimelineForSlug filters plan strides", () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    strides: [
      stride("skill", "skill deploy [keep] created", 100),
      stride("skill", "skill lint [discard] noisy", 200),
      stride("skill", "skill deploy [keep] tightened", 300),
    ],
  };
  const rows = skillTimelineForSlug(plan, "deploy");
  assert(rows.length === 2, "two deploy ledger strides");
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`ok ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`FAIL ${name}:`, e);
    }
  }
  if (failed > 0) process.exit(1);
})();
