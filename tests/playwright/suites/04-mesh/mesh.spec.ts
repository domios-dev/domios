/**
 * 04-mesh — peer lifecycle against the live dev daemon using the §4.2
 * peer simulator. Locks in the T3 liveness model end-to-end:
 * pair → online → wedged peer (TCP-accepts, never answers, the #130
 * scenario) flips offline only after the 2-miss hysteresis → recovers to
 * online when the peer answers again.
 *
 * API-level only in V1 — Mesh-tab UI assertions follow after the
 * data-testid pass gives the canvas stable selectors.
 */
import { expect, test } from "@playwright/test";
import { ApiClient } from "../../helpers/api-client";
import { MeshStub } from "../../helpers/mesh-stub";

const api = new ApiClient();

interface MeshPeer {
  instance_id: string;
  instance_name: string;
  online: boolean;
}
interface MeshPeers {
  paired: MeshPeer[];
  discovered: { instance_id: string }[];
}

async function pairedPeer(id: string): Promise<MeshPeer | undefined> {
  const peers = await api.get<MeshPeers>("/mesh/peers");
  return peers.paired.find((p) => p.instance_id === id);
}

async function pollUntil<T>(
  label: string,
  timeoutMs: number,
  intervalMs: number,
  probe: () => Promise<T | undefined>,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await probe();
    if (value !== undefined) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for: ${label}`);
    await new Promise((r) => setTimeout(r, intervalMs));
  }
}

test("@golden stub peer: pair → online → wedged → offline (hysteresis) → recovered", async () => {
  // Liveness transitions ride the 15s health loop with a 2-miss threshold;
  // the full cycle needs several loop rounds.
  test.setTimeout(420_000);

  const mesh = await api.get<{ enabled: boolean }>("/mesh/status");
  test.skip(!mesh.enabled, "mesh disabled in dev config");

  const stub = new MeshStub({ services: 3 });
  let peerId: string | undefined;

  try {
    await stub.start("happy");

    // Manual discovery: the daemon assigns the stub a manual-<uuid> id,
    // which the stub must echo in its pair-accept (identity guard).
    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    peerId = added.instance_id;
    stub.instanceId = peerId;

    await api.post("/mesh/connect", { instance_id: peerId });

    // Pairing completes via the stub's async accept callback.
    const paired = await pollUntil("stub appears in paired list", 20_000, 500, () =>
      pairedPeer(peerId!),
    );
    expect(paired.online, "freshly paired peer must start online").toBe(true);
    expect(
      stub.requests.some((r) => r === "POST /api/mesh/pair/request"),
      `stub never saw the pairing request — saw: ${stub.requests.join(", ")}`,
    ).toBe(true);

    // Survive at least one full health-loop round while answering pings.
    await new Promise((r) => setTimeout(r, 20_000));
    expect((await pairedPeer(peerId))?.online, "healthy peer flapped offline").toBe(true);
    expect(
      stub.requests.some((r) => r === "GET /api/mesh/ping"),
      "daemon never pinged the stub",
    ).toBe(true);

    // Wedge: TCP accepts, nothing answers — the #130 scenario. The flag
    // must flip offline (2 consecutive missed rounds ≈ 30-45s), and must
    // do so without wedging the daemon itself.
    stub.mode = "wedged";
    await pollUntil("wedged peer flips offline", 120_000, 3_000, async () =>
      (await pairedPeer(peerId!))?.online === false ? true : undefined,
    );

    // Daemon is still responsive while the peer hangs (no event-loop wedge).
    const health = await api.health();
    expect(health.status).toBe("ok");

    // Recovery: answering again brings it back online on the next round.
    stub.mode = "happy";
    await pollUntil("recovered peer flips back online", 120_000, 3_000, async () =>
      (await pairedPeer(peerId!))?.online === true ? true : undefined,
    );
  } finally {
    if (peerId) {
      await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    }
    await stub.stop();
    if (peerId) {
      const leftover = await pairedPeer(peerId).catch(() => undefined);
      expect(leftover, "cleanup must unpair the stub").toBeUndefined();
    }
  }
});

test("@golden liar peer: alive but its services endpoint errors honestly", async () => {
  test.setTimeout(120_000);
  const mesh = await api.get<{ enabled: boolean }>("/mesh/status");
  test.skip(!mesh.enabled, "mesh disabled in dev config");

  const stub = new MeshStub({ services: 3 });
  let peerId: string | undefined;
  try {
    await stub.start("liar");
    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    peerId = added.instance_id;
    stub.instanceId = peerId;
    await api.post("/mesh/connect", { instance_id: peerId });
    await pollUntil("liar pairs", 20_000, 500, () => pairedPeer(peerId!));

    // The stub's own surface behaves as scripted: ping fine, services 500.
    const ping = await fetch(`http://127.0.0.1:${stub.port}/api/mesh/ping`);
    expect(ping.status).toBe(200);
    const services = await fetch(`http://127.0.0.1:${stub.port}/api/services`);
    expect(services.status).toBe(500);
  } finally {
    if (peerId) await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    await stub.stop();
  }
});
