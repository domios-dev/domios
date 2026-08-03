/**
 * WHAT: a projected remote tab actually mounts and renders in the live dev UI —
 *       PTY output (G1/mount), authoritative chat snapshot + live event (G3), and
 *       chat-channel failure leaving the Terminal usable without blanking (G4).
 * WHY:  guards the remote-tab UI transport regression (plan pl_nlww71): a failure
 *       in the remote chat channel must not tear down or blank the remote Terminal.
 * ORACLE: asserts xterm DOM text (a11y/rows), the chat transcript DOM, and the
 *       real per-channel transport-state events — never canvas pixels.
 */
import { expect, test, API_BASE } from "../../helpers/fixtures";
import { MeshStub } from "../../helpers/mesh-stub";
import { pollUntil } from "../../helpers/poll";
import type { Page } from "@playwright/test";

interface MeshPeer {
  instance_id: string;
  instance_name: string;
  online: boolean;
}

async function pairedPeer(
  api: { get<T>(p: string): Promise<T> },
  id: string,
): Promise<MeshPeer | undefined> {
  const peers = await api.get<{ paired: MeshPeer[] }>("/mesh/peers");
  return peers.paired.find((p) => p.instance_id === id);
}

interface TransportEvent {
  channel: string;
  availability: string;
}

async function installTransportRecorder(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __pwTransport?: TransportEvent[] };
    w.__pwTransport ??= [];
    window.addEventListener("codeterm-remote-transport-state", (event) => {
      const detail = (event as CustomEvent).detail as { channel: string; availability: string };
      // Push through the live window ref so a length-reset in the test is seen.
      (w.__pwTransport ??= []).push({ channel: detail.channel, availability: detail.availability });
    });
  });
}

async function clearTransport(page: Page): Promise<void> {
  await page.evaluate(() => {
    const arr = (window as unknown as { __pwTransport?: TransportEvent[] }).__pwTransport;
    if (arr) arr.length = 0;
  });
}

async function transportEvents(page: Page): Promise<TransportEvent[]> {
  return page.evaluate(() => (window as unknown as { __pwTransport?: TransportEvent[] }).__pwTransport ?? []);
}

async function setMode(page: Page, composite: string, mode: "terminal" | "chat"): Promise<void> {
  // Drive the tab's view via its own mode event (what the Terminal|Chat switch
  // dispatches) — clicking the segmented control races its re-render animation.
  await page.evaluate(
    ({ tabId, next }) => {
      window.dispatchEvent(new CustomEvent("codeterm-client-tab-ui-mode", { detail: { tabId, mode: next } }));
    },
    { tabId: composite, next: mode },
  );
}

// Scope to the terminal-tab CONTENT root; `data-tab-id` alone also matches the
// sidebar tab-item (which holds no xterm).
const termScope = (composite: string) => `[data-testid="terminal-tab"][data-tab-id="${composite}"]`;

/** Read xterm's DOM text (a11y tree + rendered rows) for a tab — build-agnostic,
 *  no canvas pixels, no reliance on a dev-only terminal handle. */
async function terminalDomText(page: Page, composite: string): Promise<string> {
  return page.evaluate((scope) => {
    const tab = document.querySelector(scope);
    if (!tab) return "";
    const roots = Array.from(tab.querySelectorAll(".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows"));
    return roots
      .filter((el) => el.tagName !== "STYLE" && el.tagName !== "SCRIPT")
      .map((el) => (el as HTMLElement).innerText || el.textContent || "")
      .join("\n");
  }, termScope(composite));
}

async function waitForTerminalText(page: Page, composite: string, marker: string, timeout = 20_000): Promise<void> {
  await page.waitForFunction(
    ({ scope, needle }) => {
      const tab = document.querySelector(scope);
      if (!tab) return false;
      const roots = Array.from(tab.querySelectorAll(".xterm-accessibility, .xterm-accessibility-tree, .xterm-rows"));
      return roots
        .filter((el) => el.tagName !== "STYLE" && el.tagName !== "SCRIPT")
        .some((el) => ((el as HTMLElement).innerText || el.textContent || "").includes(needle));
    },
    { scope: termScope(composite), needle: marker },
    { timeout, polling: 200 },
  );
}

test("@golden remote-tab render: mount PTY + chat snapshot/live (G3) + chat-fail keeps Terminal usable (G4)", async ({ api, ui, tabCleanup }) => {
  test.setTimeout(180_000);
  const page = ui.page;

  const mesh = await api.get<{ enabled: boolean }>("/mesh/status");
  test.skip(!mesh.enabled, "mesh disabled in dev config");

  const stub = new MeshStub({ services: 1, daemonBase: API_BASE });
  // The daemon remaps this manual placeholder to the stub's own UUID on pairing.
  let placeholderId: string | undefined;
  const peerId = stub.instanceId;
  const composite = `mesh:${peerId}/${stub.remoteTabId}`;
  let orchestratorTabId: string | undefined;

  try {
    await stub.start("happy");

    // Pair the stub peer (manual add → connect → async accept).
    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    placeholderId = added.instance_id;

    await api.post("/mesh/connect", { instance_id: placeholderId });
    const paired = await pollUntil(() => pairedPeer(api, peerId), 20_000, 400, "stub pairs + comes online");
    expect(paired?.online, "freshly paired stub must be online").toBe(true);

    // A local controller tab so the daemon projects the peer's tab under it.
    const orchestrator = await tabCleanup.createTab<{ id: string }>();
    orchestratorTabId = orchestrator.id;
    await api.post("/remote-workers", {
      orchestrator_tab: orchestratorTabId,
      peer: peerId,
      remote_tab: stub.remoteTabId,
      title: "PW Remote Shared",
    });

    // Advertise our relay protocol version so the daemon will open versioned
    // relay-stream / relay-to connections to the stub (PTY + chat channels).
    await stub.announce();

    // Force a projection refresh (mesh-state-changed) and wait for the composite
    // tab to land in the sidebar.
    await installTransportRecorder(page);
    const tabItem = page.locator(`[data-testid="tab-item"][data-tab-id="${composite}"]`);
    await pollUntil(
      async () => {
        await api.post("/mesh/sync", { peers: [], generation: 0 }).catch(() => {});
        return (await tabItem.count()) > 0;
      },
      30_000,
      1_000,
      "projected remote tab appears in the sidebar",
    );
    await tabItem.first().click({ timeout: 15_000 });

    await test.step("G1/mount: Terminal mounts and the remote PTY stream delivers bytes", async () => {
      await setMode(page, composite, "terminal");
      await expect(page.locator(`${termScope(composite)} [data-testid="terminal-xterm-host"]`)).toBeVisible({ timeout: 15_000 });
      stub.emitPty("PW-PTY-INIT\r\n");
      await waitForTerminalText(page, composite, "PW-PTY-INIT");
      stub.emitPty("PW-PTY-LIVE\r\n");
      await waitForTerminalText(page, composite, "PW-PTY-LIVE");
    });

    await test.step("G3/chat: remote chat renders an authoritative snapshot and a live event", async () => {
      await setMode(page, composite, "chat");
      const messages = page.locator(`${termScope(composite)} [data-testid="chat-message"]`);
      await expect(messages.filter({ hasText: "PW-CHAT-SNAPSHOT" })).toHaveCount(1, { timeout: 20_000 });
      stub.emitChat("PW-CHAT-LIVE", "pw-chat-live");
      await expect(messages.filter({ hasText: "PW-CHAT-LIVE" })).toHaveCount(1, { timeout: 20_000 });
    });

    await test.step("G4/channel-separation: chat failure reconnects while the Terminal stays usable", async () => {
      await clearTransport(page);
      stub.failChat();

      // Chat channel degrades to reconnecting…
      await pollUntil(
        async () => (await transportEvents(page)).some((e) => e.channel === "chat" && e.availability === "reconnecting"),
        20_000,
        250,
        "chat channel publishes reconnecting after stream failure",
      );
      // …but the transcript is not blanked — prior messages remain.
      await expect(
        page.locator(`${termScope(composite)} [data-testid="chat-message"]`).filter({ hasText: "PW-CHAT-SNAPSHOT" }),
      ).toHaveCount(1);

      // Terminal remains fully usable: switch back, output resumes (cursor-replay
      // is not blank) and a fresh PTY frame is delivered while chat stays failed.
      await setMode(page, composite, "terminal");
      await expect(page.locator(`${termScope(composite)} [data-testid="terminal-xterm-host"]`)).toBeVisible({ timeout: 15_000 });
      await waitForTerminalText(page, composite, "PW-PTY-INIT");
      stub.emitPty("PW-PTY-AFTER-CHATFAIL\r\n");
      await waitForTerminalText(page, composite, "PW-PTY-AFTER-CHATFAIL");

      const events = await transportEvents(page);
      expect(
        events.some((e) => e.channel === "output" && e.availability !== "available"),
        `output channel must stay available; saw: ${JSON.stringify(events)}`,
      ).toBe(false);
      expect(await terminalDomText(page, composite), "terminal must not be blank after chat failure").toContain("PW-PTY-INIT");
    });
  } finally {
    await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    await api
      .request("DELETE", "/remote-workers", { remote_tab: stub.remoteTabId, peer: peerId })
      .catch(() => {});
    await stub.stop();
  }
});
