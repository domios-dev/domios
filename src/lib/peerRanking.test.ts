// Run: npx tsx src/lib/peerRanking.test.ts

import {
  formatLastActiveBadge,
  pickResumePeer,
  rankPeersByLastActive,
} from "./peerRanking";

let failed = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(e);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("rankPeersByLastActive sorts desc, None sinks", () => {
  const peers = [
    { instance_id: "a", instance_name: "A", last_active_ms: 100 },
    { instance_id: "b", instance_name: "B", last_active_ms: 500 },
    { instance_id: "c", instance_name: "C", last_active_ms: null },
  ];
  const ranked = rankPeersByLastActive(peers);
  assert(
    ranked.map((p) => p.instance_id).join(",") === "b,a,c",
    `expected b,a,c got ${ranked.map((p) => p.instance_id).join(",")}`,
  );
});

test("formatLastActiveBadge formats relative age", () => {
  const now = 1_000_000;
  assert(formatLastActiveBadge(now - 30_000, now) === "active now", "now");
  assert(formatLastActiveBadge(now - 120_000, now) === "active 2m ago", "2m");
  assert(formatLastActiveBadge(null, now) === null, "null");
});

test("pickResumePeer when remote is more recent than local", () => {
  const peers = [
    { instance_id: "remote", instance_name: "MacBook", last_active_ms: 1_000_000 },
  ];
  const pick = pickResumePeer(peers, 900_000, false, {
    hasRelayTarget: false,
    dismissedThisSession: false,
    nowMs: 1_010_000,
  });
  assert(pick?.instance_id === "remote", "expected remote");
});

test("pickResumePeer suppressed when relay set or local newer", () => {
  const peers = [
    { instance_id: "remote", instance_name: "MacBook", last_active_ms: 1_000_000 },
  ];
  assert(
    pickResumePeer(peers, 900_000, false, {
      hasRelayTarget: true,
      dismissedThisSession: false,
      nowMs: 1_010_000,
    }) === null,
    "relay",
  );
  assert(
    pickResumePeer(peers, 999_500, false, {
      hasRelayTarget: false,
      dismissedThisSession: false,
      nowMs: 1_010_000,
    }) === null,
    "local newer",
  );
});

if (failed > 0) {
  console.error(`\n${failed} failed`);
  process.exit(1);
}
console.log("\nall passed");
