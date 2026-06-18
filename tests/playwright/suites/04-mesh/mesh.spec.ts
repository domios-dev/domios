/**
 * 04-mesh — peer lifecycle against the live dev daemon using the §4.2
 * peer simulator. Locks in the T3 liveness model end-to-end:
 * pair → online → wedged peer (TCP-accepts, never answers, the #130
 * scenario) flips offline only after the 2-miss hysteresis → recovers to
 * online when the peer answers again.
 *
 * Asserted in BOTH the API and the rendered Mesh-tab UI (data-testid
 * selectors): peer dot online state, and the T3 #119 behavior — an
 * unreachable peer's services stay listed with the 'cached' badge.
 */
import { expect, test } from "../../helpers/fixtures";
import { MeshStub } from "../../helpers/mesh-stub";

interface MeshPeer {
  instance_id: string;
  instance_name: string;
  online: boolean;
}
interface MeshPeers {
  paired: MeshPeer[];
  discovered: { instance_id: string }[];
}

async function pairedPeer(api: { get<T>(p: string): Promise<T> }, id: string): Promise<MeshPeer | undefined> {
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

test("@golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI", async ({ api, ui }) => {
  // Liveness transitions ride the 15s health loop with a 2-miss threshold;
  // the full cycle needs several loop rounds. The Mesh tab polls every 5s,
  // so UI assertions piggyback on the same waits.
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
      pairedPeer(api, peerId!),
    );
    expect(paired.online, "freshly paired peer must start online").toBe(true);
    expect(
      stub.requests.some((r) => r === "POST /api/mesh/pair/request"),
      `stub never saw the pairing request — saw: ${stub.requests.join(", ")}`,
    ).toBe(true);

    // Survive at least one full health-loop round while answering pings.
    await new Promise((r) => setTimeout(r, 20_000));
    expect((await pairedPeer(api, peerId))?.online, "healthy peer flapped offline").toBe(true);
    expect(
      stub.requests.some((r) => r === "GET /api/mesh/ping"),
      "daemon never pinged the stub",
    ).toBe(true);

    // Open the Mesh tab: peer row online, the stub's 3 services rendered
    // fresh (not stale) in the services drawer.
    await ui.page.locator('[data-testid="nav-mesh"]').click();
    await ui.page.locator('[data-testid="mesh-tab-network"]').click();
    const peerRow = ui.page.locator(`[data-testid="mesh-peer"][data-peer-id="${peerId}"]`);
    await expect(peerRow).toBeVisible({ timeout: 15_000 });
    await expect(peerRow).toHaveAttribute("data-online", "true", { timeout: 15_000 });

    await ui.page.locator('[data-testid="mesh-tab-map"]').click();
    await ui.page.locator('[data-testid="mesh-services-toggle"]').click();
    const stubServices = ui.page.locator(
      `[data-testid="service-row"][data-source-peer="${stub.instanceName}"]`,
    );
    await expect(stubServices).toHaveCount(3, { timeout: 20_000 });
    await expect(stubServices.first()).toHaveAttribute("data-stale", "false");

    // Wedge: TCP accepts, nothing answers — the #130 scenario. The flag
    // must flip offline (2 consecutive missed rounds ≈ 30-45s), and must
    // do so without wedging the daemon itself.
    stub.mode = "wedged";
    await pollUntil("wedged peer flips offline", 120_000, 3_000, async () =>
      (await pairedPeer(api, peerId!))?.online === false ? true : undefined,
    );

    // Daemon is still responsive while the peer hangs (no event-loop wedge).
    const health = await api.health();
    expect(health.status).toBe("ok");

    // T3 #119 in the rendered UI: the unreachable peer's services must NOT
    // vanish — still listed, flagged stale, cached badge visible.
    await expect(stubServices).toHaveCount(3, { timeout: 30_000 });
    await expect(stubServices.first()).toHaveAttribute("data-stale", "true", {
      timeout: 30_000,
    });
    await expect(
      stubServices.first().locator('[data-testid="service-stale-badge"]'),
    ).toBeVisible();

    // Recovery: answering again brings it back online — and staleness
    // clears in the UI.
    stub.mode = "happy";
    await pollUntil("recovered peer flips back online", 120_000, 3_000, async () =>
      (await pairedPeer(api, peerId!))?.online === true ? true : undefined,
    );
    await expect(stubServices.first()).toHaveAttribute("data-stale", "false", {
      timeout: 30_000,
    });
  } finally {
    // Leave the UI on the workspace view so later suites see terminals.
    await ui.page
      .locator('[data-testid="workspace-item"]')
      .first()
      .click()
      .catch(() => {});
    if (peerId) {
      await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    }
    await stub.stop();
    if (peerId) {
      const leftover = await pairedPeer(api, peerId).catch(() => undefined);
      expect(leftover, "cleanup must unpair the stub").toBeUndefined();
    }
  }
});

test("@golden liar peer: alive but its services endpoint errors honestly", async ({ api }) => {
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
    await pollUntil("liar pairs", 20_000, 500, () => pairedPeer(api, peerId!));

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
