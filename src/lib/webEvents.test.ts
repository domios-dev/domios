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

// On an `overflow` frame (D1 §3) webEvents dispatches each RESYNC_EVENTS
// entry with a null payload as a bare "go refetch now" signal — every entry
// must be confirmed payload-agnostic at its listener site(s), or a null
// dispatch would no-op or throw instead of refetching.
console.log("webEvents RESYNC_EVENTS:");
assert("resyncs plan-changed (plan-list convergence)", RESYNC_EVENTS.includes("plan-changed"));
assert("resyncs mesh-state-changed", RESYNC_EVENTS.includes("mesh-state-changed"));
assert(
  "does NOT resync config-changed (handler applies payload directly)",
  !RESYNC_EVENTS.includes("config-changed"),
);
assert(
  "does NOT resync shell-command-changed (handler reads payload.pane_id unguarded)",
  !RESYNC_EVENTS.includes("shell-command-changed"),
);
assert(
  "does NOT resync usage-snapshot-updated (handler reads payload.provider unguarded)",
  !RESYNC_EVENTS.includes("usage-snapshot-updated"),
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
