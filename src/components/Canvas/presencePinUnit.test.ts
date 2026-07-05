// Run: npx tsx src/components/Canvas/presencePinUnit.test.ts

import type { PresenceAgent } from "./presence/identity";
import { isLivePresencePin, livePresencePins } from "./presencePinUnit";
import type { PresenceOverlay } from "./presenceOverlays";

function agent(id: string): PresenceAgent {
  return { id, label: id, role: null, color: "#4f7df0" };
}

function overlay(agents: PresenceAgent[]): PresenceOverlay {
  return {
    taskId: "task-1",
    agents,
    live: true,
    label: "worker",
    action: null,
    world: "W1",
    anchor: { x: 10, y: 20 },
    apparentWidth: 120,
  };
}

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

ok("lead agent → live pin", isLivePresencePin(overlay([agent("a1")])));
ok("empty agents → not live", !isLivePresencePin(overlay([])));
ok("peer flagged as lead → not live", !isLivePresencePin(overlay([{ ...agent("peer:x"), isPeer: true }])));

const mixed = livePresencePins([
  overlay([]),
  overlay([agent("a1")]),
  overlay([agent("a2")]),
]);
ok("filters to pins with lead only", mixed.length === 2 && mixed.every((p) => p.agents[0]?.id));

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
