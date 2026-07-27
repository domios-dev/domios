// Run: npx tsx src/components/Canvas/planView/companionResolver.test.ts

import type { OrchestratorRef, PlannerRef } from "../types";
import { resolveCompanionPane, type CompanionPlanInput } from "./companionResolver";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function owner(paneId: string | null): OrchestratorRef {
  return { pane_id: paneId, session_id: null, title: null };
}

function plannerPane(paneId: string): PlannerRef {
  return { kind: "pane", pane_id: paneId };
}

function live(...ids: string[]): (paneId: string) => boolean {
  const set = new Set(ids);
  return (paneId) => set.has(paneId);
}

function plan(input: Partial<CompanionPlanInput>): CompanionPlanInput {
  return { owner: owner("orch"), ...input };
}

test("live planner Tab wins over orchestrator", () => {
  const r = resolveCompanionPane(plan({ planner: plannerPane("plan-pane") }), live("plan-pane", "orch"));
  assert(r?.kind === "planner" && r.paneId === "plan-pane", "expected planner resolution");
});

test("falls back to orchestrator when planner Tab is dead", () => {
  const r = resolveCompanionPane(plan({ planner: plannerPane("plan-pane") }), live("orch"));
  assert(r?.kind === "orchestrator" && r.paneId === "orch", "expected orchestrator fallback");
});

test("falls back to orchestrator when planner is waived", () => {
  const r = resolveCompanionPane(plan({ planner: { kind: "waived", note: "solo" } }), live("orch"));
  assert(r?.kind === "orchestrator" && r.paneId === "orch", "expected orchestrator fallback on waiver");
});

test("falls back to orchestrator when no planner is bound", () => {
  const r = resolveCompanionPane(plan({ planner: null }), live("orch"));
  assert(r?.kind === "orchestrator" && r.paneId === "orch", "expected orchestrator fallback on unbound");
});

test("resolves nothing when neither planner nor orchestrator is live", () => {
  const r = resolveCompanionPane(plan({ planner: plannerPane("plan-pane") }), live("other"));
  assert(r === null, "expected null when nothing live resolves");
});

test("resolves nothing when orchestrator Tab is unset", () => {
  const r = resolveCompanionPane({ owner: owner(null), planner: null }, live("orch"));
  assert(r === null, "expected null when owner Tab is unset");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (err) {
    failed++;
    console.error(`  ✗ ${name}: ${(err as Error).message}`);
  }
}
if (failed > 0) {
  console.error(`\n${failed} of ${tests.length} companionResolver test(s) failed`);
  process.exit(1);
}
console.log(`\n✓ all ${tests.length} companionResolver test(s) green`);
