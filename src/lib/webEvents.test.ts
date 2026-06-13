import { POLL_EVENTS } from "./webEvents";

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

// The 5s poll tick dispatches each POLL_EVENTS entry with a null payload as a
// pure "refetch now" signal. Any payload-bearing event whose handler reads
// fields off the payload (e.g. pane-status-changed → event.payload.pane_id)
// would throw on that null. Such events arrive with real data over SSE and
// must never be polled here.
console.log("webEvents POLL_EVENTS:");
assert("polls mux-state-changed (refresh signal)", POLL_EVENTS.includes("mux-state-changed"));
assert("polls mesh-state-changed (refresh signal)", POLL_EVENTS.includes("mesh-state-changed"));
assert(
  "does NOT poll pane-status-changed (payload-bearing, SSE-delivered)",
  !POLL_EVENTS.includes("pane-status-changed"),
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
