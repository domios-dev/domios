/**
 * WHAT: mesh covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 04-mesh.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { MeshStub } from "../../helpers/mesh-stub";
import { pollUntil } from "../../helpers/poll";

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

/** A crashed earlier run can leave a same-named stub paired at a dead port; drop it so
 *  `data-peer-id` selectors and the cleanup assertion stay unambiguous. */
async function dropStalePeersNamed(api: ApiClient, instanceName: string): Promise<void> {
  const peers = await api.get<MeshPeers>("/mesh/peers");
  for (const peer of peers.paired) {
    if (peer.instance_name === instanceName) {
      await api.post(`/mesh/peers/disconnect/${peer.instance_id}`).catch(() => {});
    }
  }
}

async function dismissRecoveryBannerIfPresent(page: {
  locator(selector: string): {
    click(opts?: { timeout?: number }): Promise<void>;
  };
}) {
  await page
    .locator('[data-testid="recovery-status-banner"] button[aria-label="Dismiss crash recovery"]')
    .click({ timeout: 1_000 })
    .catch(() => {});
}

test("@golden stub peer: pair → online → wedged → offline (hysteresis) → recovered, in API and Mesh-tab UI", async ({ api, ui }) => {
  // Liveness transitions ride the 15s health loop with a 2-miss threshold;
  test.setTimeout(420_000);

  const mesh = await api.get<{ enabled: boolean }>("/mesh/status");
  test.skip(!mesh.enabled, "mesh disabled in dev config");

  const stub = new MeshStub({ services: 3 });
  let peerId: string | undefined;

  try {
    await dropStalePeersNamed(api, stub.instanceName);
    await stub.start("happy");

    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    peerId = added.instance_id;
    // Pairing rejects non-UUID identities and remaps the daemon's manual-<uuid>
    // placeholder to the peer's real id — the stub must present its own UUID.
    await api.post("/mesh/connect", { instance_id: peerId });
    await stub.waitForPairAccept();

    // Pairing completes via the stub's async accept callback.
    const paired = await pollUntil(
      () => pairedPeer(api, stub.instanceId),
      20_000,
      500,
      "stub appears in paired list",
    );
    if (!paired) throw new Error("stub did not appear in paired list");
    peerId = stub.instanceId;
    expect(paired.online, "freshly paired peer must start online").toBe(true);
    expect(
      stub.requests.some((r) => r === "POST /api/mesh/pair/request"),
      `stub never saw the pairing request — saw: ${stub.requests.join(", ")}`,
    ).toBe(true);

    // Survive at least one full health-loop round while answering pings.
    await pollUntil(async () => {
      const peer = await pairedPeer(api, peerId!);
      return peer?.online === true && stub.requests.some((r) => r === "GET /api/mesh/ping");
    }, 60_000, 2_000, "healthy peer answers at least one ping");
    expect((await pairedPeer(api, peerId))?.online, "healthy peer flapped offline").toBe(true);

    // fresh (not stale) in the services drawer.
    await ui.page.locator('[data-testid="nav-mesh"]').click();
    await dismissRecoveryBannerIfPresent(ui.page);
    await ui.page.locator('[data-testid="mesh-tab-network"]').click();
    const peerRow = ui.page.locator(`[data-testid="mesh-peer"][data-peer-id="${peerId}"]`);
    await expect(peerRow).toBeVisible({ timeout: 15_000 });
    await expect(peerRow).toHaveAttribute("data-online", "true", { timeout: 15_000 });

    await ui.page.locator('[data-testid="mesh-tab-map"]').click();
    const serviceRows = ui.page.locator('[data-testid="service-row"]');
    if ((await serviceRows.count()) === 0) {
      await ui.page.locator('[data-testid="mesh-services-toggle"]').click();
    }
    const stubServices = ui.page.locator(
      `[data-testid="service-row"][data-source-peer-id="${peerId}"]`,
    );
    await expect(stubServices).toHaveCount(3, { timeout: 20_000 });
    await expect(stubServices.first()).toHaveAttribute("data-stale", "false");

    stub.mode = "wedged";
    await pollUntil(
      async () => (await pairedPeer(api, peerId!))?.online === false,
      120_000,
      3_000,
      "wedged peer flips offline",
    );

    // Daemon is still responsive while the peer hangs (no event-loop wedge).
    const health = await api.health();
    expect(health.status).toBe("ok");

    // vanish — still listed, flagged stale, cached badge visible.
    await expect(stubServices).toHaveCount(3, { timeout: 30_000 });
    await expect(stubServices.first()).toHaveAttribute("data-stale", "true", {
      timeout: 30_000,
    });
    await expect(
      stubServices.first().locator('[data-testid="service-stale-badge"]'),
    ).toBeVisible();

    // clears in the UI.
    stub.mode = "happy";
    await pollUntil(
      async () => (await pairedPeer(api, peerId!))?.online === true,
      120_000,
      3_000,
      "recovered peer flips back online",
    );
    await expect(stubServices.first()).toHaveAttribute("data-stale", "false", {
      timeout: 30_000,
    });
  } finally {
    // Leave the UI on the workspace view so later suites see terminals.
    await ui.page
      .locator('[data-testid="workspace-item"]')
      .first()
      .click({ timeout: 2_000 })
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
    await dropStalePeersNamed(api, stub.instanceName);
    await stub.start("liar");
    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    peerId = added.instance_id;
    // The stub must present its OWN uuid; the daemon remaps its manual-<uuid>
    // placeholder onto it. Overwriting it here is what produced the 502.
    await api.post("/mesh/connect", { instance_id: peerId });
    await stub.waitForPairAccept();
    await pollUntil(() => pairedPeer(api, stub.instanceId), 20_000, 500, "liar pairs");
    peerId = stub.instanceId;

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
