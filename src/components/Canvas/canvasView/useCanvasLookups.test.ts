// Run: npx tsx src/components/Canvas/canvasView/useCanvasLookups.test.ts

import { buildTaskPresenceFingerprint } from "./presenceFingerprint";
import type { Plan } from "../types";
import { milestone, plan, section, task } from "../layout/fixtures";
import type { Tab } from "../../../lib/types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const tabsA: Record<string, Tab> = {
  tabA: { id: "tabA", pty_id: null, tab_type: "Terminal", title: "Worker A", cwd: null },
};

const basePlan = plan("p", "orch", 1, [section("sec")], [
  task("t", ["sec"], {
    assigned_tab_id: "tabA",
    peer: null,
    status: "pending",
    body: "x",
    milestones: [milestone("m", "status_update")],
  }),
]);

function withStatus(p: Plan, status: string): Plan {
  return { ...p, tasks: [{ ...p.tasks[0], status }] } as Plan;
}

test("buildTaskPresenceFingerprint ignores task status churn", () => {
  const pending = [basePlan];
  const live = [withStatus(basePlan, "in_progress")];
  assert(
    buildTaskPresenceFingerprint(pending, tabsA) === buildTaskPresenceFingerprint(live, tabsA),
    "status-only changes do not change the presence build key",
  );
});

test("buildTaskPresenceFingerprint changes when presence-relevant focus changes", () => {
  const before = plan("p", "orch", 1, [section("sec")], [
    task("t", ["sec"], { assigned_tab_id: "tabA", status: "in_progress" }),
  ], { heartbeat: { current_focus: "phase 1", active_lanes: [] } });
  const after = { ...before, heartbeat: { current_focus: "phase 2", active_lanes: [] } } as Plan;
  assert(
    buildTaskPresenceFingerprint([before], tabsA) !== buildTaskPresenceFingerprint([after], tabsA),
    "heartbeat changes that affect focus invalidate the presence key",
  );
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
  console.log(`\n✓ ${passed}/${tests.length} useCanvasLookups tests passed`);
}

main();
