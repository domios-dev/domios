import { RESYNC_EVENTS } from "./webEvents";

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("webEvents RESYNC_EVENTS:");
assert("resyncs plan-changed (plan-list convergence)", RESYNC_EVENTS.includes("plan-changed"));
assert("resyncs mesh-state-changed", RESYNC_EVENTS.includes("mesh-state-changed"));
assert(
  "does NOT resync config-changed (handler applies payload directly)",
  !RESYNC_EVENTS.includes("config-changed"),
);
assert(
  "does NOT resync shell-command-changed (handler reads payload.tab_id unguarded)",
  !RESYNC_EVENTS.includes("shell-command-changed"),
);
assert(
  "does NOT resync usage-snapshot-updated (handler reads payload.provider unguarded)",
  !RESYNC_EVENTS.includes("usage-snapshot-updated"),
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
