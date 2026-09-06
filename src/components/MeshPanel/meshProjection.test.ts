// Run: npm run test:unit -- meshProjection

import { groupServicesByPeer, pairedPeers, SELF_PEER_KEY, visibleServices } from "./meshProjection";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";

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

function svc(id: string, kind: Service["kind"], sourcePeerId?: string): Service {
  return {
    id,
    name: id,
    kind,
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 0,
    updated_at: 0,
    enabled: true,
    source_peer_id: sourcePeerId,
  };
}

function peer(instanceId: string, pending: boolean): PeerStatus {
  return {
    instance_id: instanceId,
    name: instanceId,
    ip: "100.64.0.2",
    api_port: 7685,
    online: !pending,
    access_level: pending ? "pending" : "full",
    last_seen: null,
    pending: pending || undefined,
    relay_protocol_version: pending ? 0 : 1,
  };
}

// codeterm#440: a discovered-not-paired (pending) peer must never count as paired.
{
  const peers = [peer("paired-1", false), peer("pending-1", true)];
  const paired = pairedPeers(peers);
  assert("pending peer excluded from paired count", paired.length === 1);
  assert("the remaining peer is the truly-paired one", paired[0]?.instance_id === "paired-1");
}

// The exact reported shape: one discovered-not-paired peer and zero real
// services must not project as "2 peers, paired, 3 services".
{
  const peers = [peer("pending-only", true)];
  const services: Service[] = [];
  assert("zero paired peers, not 2", pairedPeers(peers).length === 0);
  assert("zero services, not 3", visibleServices(services).length === 0);
}

// Cron jobs are virtual services with their own surface — excluded from the
// count the header and every peer card share.
{
  const services = [svc("daemon-1", "daemon"), svc("cron-1", "cron")];
  const visible = visibleServices(services);
  assert("cron excluded from visible services", visible.length === 1);
  assert("the daemon is the one kept", visible[0]?.id === "daemon-1");
}

// Header equals rows: the header total must equal the sum across every
// peer's row bucket, by construction — same grouping, not two derivations.
{
  const services = [
    svc("self-daemon", "daemon"),
    svc("remote-daemon", "daemon", "peer-a"),
    svc("remote-cron", "cron", "peer-a"),
  ];
  const grouped = groupServicesByPeer(services);
  const rowTotal = Array.from(grouped.values()).reduce((sum, list) => sum + list.length, 0);
  assert(
    "header total equals sum of every rendered row",
    visibleServices(services).length === rowTotal,
    `header=${visibleServices(services).length} rows=${rowTotal}`,
  );
  assert("self bucket present", grouped.has(SELF_PEER_KEY));
  assert("self service attributed to self", grouped.get(SELF_PEER_KEY)?.some((s) => s.id === "self-daemon") === true);
  assert("remote daemon attributed to its peer", grouped.get("peer-a")?.some((s) => s.id === "remote-daemon") === true);
  assert("remote cron dropped from every bucket", !Array.from(grouped.values()).some((list) => list.some((s) => s.id === "remote-cron")));
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
