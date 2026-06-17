import {
  getMeshPresenceMenuItems,
  getMeshPresenceSummary,
  type MeshPresence,
} from "./meshPresenceHeader";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? ` — ${detail}` : ""}`);
    failed++;
  }
}

function labels(state: MeshPresence) {
  return getMeshPresenceMenuItems(state).map((item) => `${item.label}:${item.active ? "active" : "idle"}`);
}

console.log("meshPresenceHeader tests");

{
  const summary = getMeshPresenceSummary("online");
  assert("online summary label", summary.label === "Online");
  assert("online summary title", summary.title === "Mesh: online — visible to peers");
  assert("online menu marks Online active", labels("online").join(",") === "Online:active,Offline:idle");
}

{
  const summary = getMeshPresenceSummary("offline");
  assert("offline summary label", summary.label === "Offline");
  assert("offline summary title", summary.title === "Mesh: offline");
  assert("offline menu marks Offline active", labels("offline").join(",") === "Online:idle,Offline:active");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
