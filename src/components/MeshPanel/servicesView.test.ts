import {
  collectPeerErrors,
  mergeServicesView,
  type PeerServicesView,
} from "./servicesView";
import type { Service } from "./types";

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

function svc(id: string, name: string): Service {
  return {
    id,
    name,
    kind: "daemon",
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 0,
    updated_at: 0,
    enabled: true,
  };
}

function view(partial: Partial<PeerServicesView>): PeerServicesView {
  return {
    services: [],
    stale: false,
    fetched_at: null,
    peer_online: true,
    error: null,
    ...partial,
  };
}

console.log("servicesView tests");

// Regression #119: keep cached services for offline peers in merged list,
// flagged with stale/peer_offline instead of dropping them.
{
  const merged = mergeServicesView(
    [svc("local1", "local")],
    [
      {
        peerName: "rollacode-bot",
        view: view({
          services: [svc("r1", "remote-a"), svc("r2", "remote-b")],
          stale: true,
          peer_online: false,
          fetched_at: 123,
        }),
      },
    ],
  );
  assert("offline peer's cached services retained", merged.length === 3);
  const remote = merged.filter((s) => s.source_peer === "rollacode-bot");
  assert(
    "cached services flagged stale",
    remote.every((s) => s.stale === true),
  );
  assert(
    "cached services flagged peer-offline",
    remote.every((s) => s.peer_online === false),
  );
  assert(
    "local services not flagged",
    merged.find((s) => s.id === "local1")?.stale === undefined,
  );
}

// #127 regression: a failed invoke must not poison the rest of the merge.
{
  const merged = mergeServicesView(
    [svc("local1", "local")],
    [
      { peerName: "dead-peer", view: null },
      {
        peerName: "live-peer",
        view: view({ services: [svc("r1", "remote-a")] }),
      },
    ],
  );
  assert("failed peer skipped, live peer merged", merged.length === 2);
  assert(
    "fresh services not stale",
    merged.find((s) => s.id === "r1")?.stale === false,
  );
}

// Fetch errors surface per peer instead of disappearing.
{
  const errors = collectPeerErrors([
    { peerName: "a", view: view({ error: "all addresses failed" }) },
    { peerName: "b", view: view({}) },
    { peerName: "c", view: null },
  ]);
  assert("errors collected with peer prefix", errors.length === 1);
  assert(
    "error message preserved",
    errors[0] === "a: all addresses failed",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
