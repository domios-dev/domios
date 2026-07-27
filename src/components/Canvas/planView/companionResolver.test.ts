// Run: npx tsx src/components/Canvas/planView/companionResolver.test.ts

import type { OrchestratorRef, PlannerRef } from "../types";
import { resolveCompanionTab, type CompanionPlanInput } from "./companionResolver";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function owner(tabId: string | null): OrchestratorRef {
  return { tab_id: tabId, session_id: null, title: null };
}

function plannerTab(tabId: string): PlannerRef {
  return { kind: "tab", tab_id: tabId };
}

function live(...ids: string[]): (tabId: string) => boolean {
  const set = new Set(ids);
  return (tabId) => set.has(tabId);
}

function plan(input: Partial<CompanionPlanInput>): CompanionPlanInput {
  return { owner: owner("orch"), ...input };
}

test("live planner Tab wins over orchestrator", () => {
  const r = resolveCompanionTab(plan({ planner: plannerTab("plan-tab") }), live("plan-tab", "orch"));
  assert(r?.kind === "planner" && r.tabId === "plan-tab", "expected planner resolution");
});

test("falls back to orchestrator when planner Tab is dead", () => {
  const r = resolveCompanionTab(plan({ planner: plannerTab("plan-tab") }), live("orch"));
  assert(r?.kind === "orchestrator" && r.tabId === "orch", "expected orchestrator fallback");
});

test("falls back to orchestrator when planner is waived", () => {
  const r = resolveCompanionTab(plan({ planner: { kind: "waived", note: "solo" } }), live("orch"));
  assert(r?.kind === "orchestrator" && r.tabId === "orch", "expected orchestrator fallback on waiver");
});

test("falls back to orchestrator when no planner is bound", () => {
  const r = resolveCompanionTab(plan({ planner: null }), live("orch"));
  assert(r?.kind === "orchestrator" && r.tabId === "orch", "expected orchestrator fallback on unbound");
});

test("resolves nothing when neither planner nor orchestrator is live", () => {
  const r = resolveCompanionTab(plan({ planner: plannerTab("plan-tab") }), live("other"));
  assert(r === null, "expected null when nothing live resolves");
});

test("resolves nothing when orchestrator Tab is unset", () => {
  const r = resolveCompanionTab({ owner: owner(null), planner: null }, live("orch"));
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
