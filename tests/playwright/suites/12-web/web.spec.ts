/**
 * WHAT: web covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import { MeshStub } from "../../helpers/mesh-stub";
import { pollUntil } from "../../helpers/poll";
import { tempSpawnCwd } from "../../helpers/spawn-cwd";
import { createStubWorkspace, isStubTransportActive } from "../../helpers/stub-gate";

const WEB_URL = `${API_BASE}/`;

async function newWebPage(browser: Browser, viewport?: { width: number; height: number }): Promise<Page> {
  const ctx = await browser.newContext({ viewport: viewport ?? { width: 1280, height: 800 } });
  return ctx.newPage();
}

async function passTokenGate(page: Page): Promise<void> {
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 15_000 });
  await page.fill('[data-testid="web-token-input"]', readDevToken());
  await page.click('[data-testid="web-token-submit"]');
  // additionally boot with the sidebar collapsed behind a floating hamburger.
  await page
    .locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]')
    .first()
    .waitFor({ timeout: 20_000 });
}

test("@smoke / serves the SPA; token gate authenticates; self is the default host", async ({ api, webBrowser }) => {
  const page = await newWebPage(webBrowser);
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await passTokenGate(page);

  // Default target is the origin that served the bundle.
  const host = await page.evaluate(() => sessionStorage.getItem("codeterm-web-host"));
  expect(host).toBeNull();

  // The picker labels the current host with the daemon-reported local
  // instance name (GET /api/mesh/peers), proving the default target is self.
  const mesh = await api.get<{ local_instance_name: string }>("/mesh/peers");
  await expect(page.locator('[data-testid="peer-picker-button"]')).toBeVisible();
  await expect(page.locator('[data-testid="peer-picker-button"]')).toContainText(mesh.local_instance_name);
  await page.context().close();
});

test("@golden bad token is rejected with a rendered error, not a broken app", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser);
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]');
  await page.fill('[data-testid="web-token-input"]', "definitely-wrong-token");
  await page.click('[data-testid="web-token-submit"]');
  const tokenError = page.locator('[data-testid="web-token-error"]');
  await expect(tokenError).toBeVisible({ timeout: 10_000 });
  await expect(tokenError).toContainText(/token|auth|invalid|unauthorized/i);
  await page.context().close();
});

test("@golden peer switching: dropdown lists the stub, traffic routes to it, switch-back restores", async ({ api, webBrowser }) => {
  test.setTimeout(240_000);
  const STUB_TOKEN = "stub-web-token-123";
  const stub = new MeshStub({ services: 1, webToken: STUB_TOKEN, daemonBase: API_BASE });
  stub.muxDelayMs = 500;
  let peerId: string | undefined;
  const selfWorkspaceName = `${stub.instanceName}-self-host`;
  let selfWorkspaceId: string | undefined;

  // before its cleanup — drop them so selectors stay unambiguous.
  const existing = await api.get<{ paired: { instance_id: string; instance_name: string }[] }>(
    "/mesh/peers",
  );
  for (const p of existing.paired) {
    if (p.instance_name === stub.instanceName) {
      await api.post(`/mesh/peers/disconnect/${p.instance_id}`).catch(() => {});
    }
  }

  try {
    await api.createWorkspace(selfWorkspaceName);
    selfWorkspaceId = (await api.workspaces()).find((w) => w.name === selfWorkspaceName)?.id;
    // has every field it expects when pointed at the stub.
    stub.configPayload = await api.get("/config");
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
    await pollUntil(async () => {
      const peers = await api.get<{ paired: { instance_id: string }[] }>("/mesh/peers");
      return peers.paired.some((p) => p.instance_id === stub.instanceId);
    }, 20_000, 500, "stub peer pairs for web switching");
    peerId = stub.instanceId;
    // The version-skew guard disables peers that have not announced the
    // current relay protocol version.
    await stub.announce();

    const page = await newWebPage(webBrowser);
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    await passTokenGate(page);

    // Dropdown lists the stub with its online state.
    await page.click('[data-testid="peer-picker-button"]');
    const item = page.locator(`[data-testid="peer-picker-item"][data-peer-name="${stub.instanceName}"]`);
    await expect(item).toBeVisible({ timeout: 15_000 });
    await expect(item).toHaveAttribute("data-peer-online", "true");

    const oldTabId = await page.locator("[data-tab-id]").first().getAttribute("data-tab-id");
    expect(oldTabId, "self host should expose an active tab before switching").toBeTruthy();

    const before = stub.requests.length;
    await item.click();
    await expect(page.locator('[data-testid="web-token-input"]')).toHaveCount(0);
    await expect(page.locator(`[data-tab-id="${oldTabId}"]`)).toHaveCount(0);

    // Prove WHICH state we reached before asserting how it renders, so a missing
    // card cannot masquerade as a switch that never happened.
    await expect(
      page.locator('[data-testid="peer-picker-button"]'),
      "host switch never landed on the stub peer",
    ).toContainText(stub.instanceName, { timeout: 20_000 });

    // A peer reports a protected General plus ordinary workspaces; the sidebar
    // partitions on `protected`, so only the ordinary one is a card.
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${stub.workspaceName}"]`),
      "peer's ordinary workspace did not render as a sidebar workspace card",
    ).toBeVisible({ timeout: 20_000 });
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${stub.generalWorkspaceName}"]`),
      "peer's protected General belongs in the general section, never in a card",
    ).toHaveCount(0);
    await expect(
      page.locator(`[data-testid="general-section"][aria-label="${stub.generalWorkspaceName} workspace contents"]`),
      "peer's protected General did not render as the general section",
    ).toHaveCount(1);
    expect(stub.requests.length, "stub saw no web traffic after switch").toBeGreaterThan(before);
    expect(
      stub.requests.some(
        (r) =>
          r.includes("/api/mux/state") ||
          r.includes("/api/workspaces") ||
          r.includes("/api/mesh/relay"),
      ),
      `stub requests: ${stub.requests.slice(-10).join(", ")}`,
    ).toBe(true);
    expect(stub.requests.filter((r) => r.includes("/api/mesh/relay")).length).toBeGreaterThanOrEqual(
      1,
      `stub requests: ${stub.requests.slice(-10).join(", ")}`,
    );
    expect(
      stub.requests.slice(before).filter((request) => request.includes(`/api/tabs/${oldTabId}/`)),
      "new peer received a request for the old peer tab",
    ).toEqual([]);

    // Switch back to self: real workspaces return, no stub data leaks. Wait on
    // THIS host's own ordinary workspace — a fresh instance holds only the
    // protected General, which renders in general-section and never as a card.
    await page.click('[data-testid="peer-picker-button"]');
    await page.click('[data-testid="peer-picker-self"]');
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${selfWorkspaceName}"]`),
      "self host's own workspace did not return after switch-back",
    ).toBeVisible({ timeout: 20_000 });
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${stub.workspaceName}"]`),
      "stub workspace leaked into the self host after switch-back",
    ).toHaveCount(0);
    const host = await page.evaluate(() => sessionStorage.getItem("codeterm-web-host"));
    expect(host, "switch-back must clear the host override").toBeNull();
    await page.context().close();
  } finally {
    if (peerId) await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    if (selfWorkspaceId) await api.closeWorkspace(selfWorkspaceId).catch(() => {});
    await stub.stop();
  }
});

const RELAY_KEY = "codeterm-web-relay-peer";

async function seedGateStorage(page: Page, token: string, relayPeerId: string): Promise<void> {
  await page.evaluate(
    ({ token: value, relayPeerId: peer }) => {
      const self = window.location.origin;
      localStorage.setItem(`codeterm-web-token:${self}`, value);
      localStorage.setItem(`codeterm-web-relay-peer:${self}`, peer);
      localStorage.setItem(`codeterm-web-relay-peer-name:${self}`, "unreachable peer");
    },
    { token, relayPeerId },
  );
}

function storedRelayPeer(page: Page): Promise<string | null> {
  return page.evaluate(
    (key) => localStorage.getItem(`${key}:${window.location.origin}`),
    RELAY_KEY,
  );
}

test("@golden stale relay target is dropped at boot; when it survives, back-to-self recovers", async ({ api, webBrowser }) => {
  // A target left in localStorage outlives pairing, presence and protocol. The
  // gate re-checks it against the LOCAL daemon's paired list and discards it
  // before anything can be relayed into a dead peer (WebGate.tsx bootstrap).
  const dropped = await newWebPage(webBrowser);
  try {
    await dropped.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    await seedGateStorage(dropped, readDevToken(), "unreachable-peer");
    await dropped.reload({ waitUntil: "domcontentloaded" });

    const mesh = await api.get<{ local_instance_name: string }>("/mesh/peers");
    await expect(
      dropped.locator('[data-testid="peer-picker-button"]'),
      "a stale relay target must not strand the user at the gate",
    ).toBeVisible({ timeout: 20_000 });
    await expect(dropped.locator('[data-testid="web-token-input"]')).toHaveCount(0);
    await expect(dropped.locator('[data-testid="peer-picker-button"]')).toContainText(
      mesh.local_instance_name,
    );
    expect(await storedRelayPeer(dropped), "stale relay target survived the boot check").toBeNull();
  } finally {
    await dropped.context().close();
  }

  // When the daemon cannot be asked WHICH peers are usable, the gate keeps the
  // stored target rather than guessing — so a rejected token must still offer the
  // way back, and taking it must clear the target.
  const stranded = await newWebPage(webBrowser);
  try {
    await stranded.route(/\/api\/mesh\/peers(\?|$)/, (route) => route.abort());
    await stranded.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    await seedGateStorage(stranded, "definitely-wrong-token", "unreachable-peer");
    await stranded.reload({ waitUntil: "domcontentloaded" });

    await expect(stranded.locator('[data-testid="web-token-error"]')).toBeVisible({ timeout: 20_000 });
    const backToSelf = stranded.locator('[data-testid="web-token-back-to-self"]');
    await expect(backToSelf).toContainText("Back to");
    await backToSelf.click();

    await expect(stranded.locator('[data-testid="web-token-input"]')).toBeVisible({ timeout: 20_000 });
    await expect(
      stranded.locator('[data-testid="web-token-back-to-self"]'),
      "back-to-self must disappear once the target is cleared",
    ).toHaveCount(0);
    expect(await storedRelayPeer(stranded), "back-to-self must clear the relay target").toBeNull();
  } finally {
    await stranded.context().close();
  }
});

test("@golden mobile viewport: no horizontal scroll, sidebar overlays, picker reachable", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser, { width: 390, height: 844 });
  try {
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    await passTokenGate(page);

    const scrollW = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollW, "horizontal scroll on phone viewport").toBeLessThanOrEqual(390);

    const burger = page.locator('[data-testid="web-sidebar-open"]');
    const picker = page.locator('[data-testid="peer-picker-button"]');
    await expect(burger).toBeVisible({ timeout: 10_000 });
    // The picker moved into the sidebar's GENERAL header (Sidebar.tsx), and phones
    // do not mount the sidebar until the hamburger asks for it (App.tsx) — so it is
    // reachable through the drawer, never from the titlebar row.
    await expect(picker, "phone titlebar must not carry the peer picker").toHaveCount(0);

    const burgerBox = await burger.boundingBox();
    const optionalBox = async (selector: string) => {
      const control = page.locator(selector).first();
      return (await control.isVisible().catch(() => false)) ? control.boundingBox() : null;
    };
    const reconnectBox = await optionalBox('[data-testid="chat-reconnect"]');
    const modeBox = await optionalBox('[data-testid="tab-mode-switch"]');
    const overlaps = (
      a: { x: number; y: number; width: number; height: number },
      b: { x: number; y: number; width: number; height: number },
    ) => a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
    if (burgerBox && reconnectBox) expect(overlaps(burgerBox, reconnectBox), "hamburger overlaps reconnect").toBe(false);
    if (burgerBox && modeBox) expect(overlaps(burgerBox, modeBox), "hamburger overlaps mode switch").toBe(false);

    const navMesh = page.locator('[data-testid="nav-mesh"]');
    await burger.click();
    await expect(picker, "peer picker unreachable from the phone drawer").toBeVisible({ timeout: 10_000 });
    const pickerBox = await picker.boundingBox();
    expect(pickerBox, "peer picker has no box in the open drawer").toBeTruthy();
    if (pickerBox) {
      expect(pickerBox.x, "peer picker left").toBeGreaterThanOrEqual(-1);
      expect(pickerBox.x + pickerBox.width, "peer picker right").toBeLessThanOrEqual(391);
    }
    // The drawer overlays; it must never widen the document.
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
      "open drawer widened the phone viewport",
    ).toBeLessThanOrEqual(390);

    await expect(navMesh).toBeVisible({ timeout: 5_000 });
    await navMesh.click();
    await expect(page.locator('[data-testid="mesh-tab-network"]')).toBeVisible({ timeout: 10_000 });
  } finally {
    await page.context().close();
  }
});

test("@golden mobile sidebar stays open for workspace cards and closes for tabs", async ({ api, webBrowser, tabCleanup }) => {
  const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const firstName = `pw-mobile-workspace-a-${suffix}`;
  const secondName = `pw-mobile-workspace-b-${suffix}`;
  await api.createWorkspace(firstName);
  await api.createWorkspace(secondName);
  const workspaces = await api.workspaces();
  const first = workspaces.find((workspace) => workspace.name === firstName);
  const second = workspaces.find((workspace) => workspace.name === secondName);
  if (!first || !second) throw new Error("mobile sidebar test workspaces were not created");
  const tab = await tabCleanup.createTab<{ tab_id: string }>({
    workspace_id: second.id,
    title: `pw-mobile-tab-${suffix}`,
  });
  await api.focusWorkspace(first.id);

  const page = await newWebPage(webBrowser, { width: 390, height: 844 });
  try {
    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    const burger = page.locator('[data-testid="web-sidebar-open"]');
    await expect(burger).toBeVisible({ timeout: 20_000 });
    await burger.click();

    const drawer = page.locator(".ct-sidebar-drawer.is-open");
    await expect(drawer).toBeVisible({ timeout: 10_000 });
    await drawer
      .getByRole("button", { name: `Select workspace ${secondName}`, exact: true })
      .and(page.locator("[aria-pressed]"))
      .click();

    await expect(drawer, "workspace selection keeps the mobile drawer open").toBeVisible();
    const region = drawer.getByRole("region", { name: `${secondName} workspace contents` });
    await expect(region, "workspace selection reveals its tabs in the open drawer").toBeVisible();
    await region.locator(`[data-testid="tab-item"][data-tab-id="${tab.tab_id}"]`).click();

    await expect(page.locator(".ct-sidebar-drawer.is-open"), "tab selection dismisses the mobile drawer").toHaveCount(0);
    await expect(burger, "tab selection restores the mobile drawer trigger").toBeVisible();
  } finally {
    await page.context().close();
    await api.closeWorkspace(first.id).catch(() => {});
    await api.closeWorkspace(second.id).catch(() => {});
  }
});

test("@golden fully live on mobile: General Agent responds, read from the rendered DOM", async ({ api, webBrowser, tabCleanup, spawnAgent, pollMarkerInDom }) => {
  test.setTimeout(300_000);
  test.skip(
    !(await isStubTransportActive(api)),
    "dev daemon is not running with CODETERM_STUB_AGENT_CMD",
  );
  const page = await newWebPage(webBrowser, { width: 375, height: 812 });
  // Runtime a11y opt-in gives the browser terminal a readable DOM.
  await page.goto(`${WEB_URL}?a11y=1`, { waitUntil: "domcontentloaded" });
  await passTokenGate(page);

  const marker = `PW-WEB-${Date.now()}`;
  const expected = `ack ${marker}`;
  // Drive the spawn through the API (deterministic), observe in the DOM.
  const workspace = await createStubWorkspace(api);
  const inbox = await tabCleanup.createTab({ title: "pw-web-inbox", workspace_id: workspace.id });
  let agentTabId: string | undefined;
  try {
    await api.post(`/tabs/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await spawnAgent({
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-web-agent",
      parent_tab_id: inbox.id,
      report_target_tab_id: inbox.id,
      skip_permissions: true,
      activate: true,
      cwd: tempSpawnCwd("pw-web-agent-"),
      task: `STUB_ECHO ${marker}`,
    });
    agentTabId = spawned.tab_id;

    // render as a chat bubble instead of xterm a11y text — accept either.
    await pollMarkerInDom(expected, { page, includeChatMessages: true });
    await expect(page.locator(`[data-testid="tab-session-chat"][data-tab-id="${agentTabId!}"]`)).toContainText(expected);
  } finally {
    if (agentTabId) tabCleanup.track(agentTabId);
    await page.context().close();
  }
});

// ── share-link token auto-consume ────────────────────────────────────

test("@golden share link: #token= auto-enters and the URL is redacted", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser);
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  // No manual gate — straight to the app.
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
  const url = page.url();
  expect(url, "token leaked in the URL after consume").not.toContain("token=");
  await page.context().close();
});

test("@golden share link: ?token= query form also works and is redacted", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser);
  await page.goto(`${WEB_URL}?token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
  expect(page.url()).not.toContain("token=");
  await page.context().close();
});

test("@golden share link: a bad token falls through to the gate error path", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser);
  await page.goto(`${WEB_URL}#token=definitely-bad-token`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 20_000 });
  await expect(page.locator('[data-testid="web-token-error"]')).toBeVisible({ timeout: 10_000 });
  expect(page.url()).not.toContain("token=");
  await page.context().close();
});

// ── web chrome strip + mobile viewport hardening ─────────────────────

test("@golden web chrome: titlebar hosts keep-awake, the peer picker sits in the sidebar GENERAL header", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser);
  try {
    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });

    // asserts the same for the Tauri window).
    const titlebar = page.locator('[data-testid="titlebar"]');
    await expect(titlebar).toBeVisible();
    // Keep-awake lives in the titlebar tray; the host switcher moved down into the
    // sidebar's GENERAL structural header (Sidebar.tsx renders <WebPeerPicker compact/>
    // there), so the titlebar must no longer carry it.
    await expect(titlebar.locator('[data-testid="keepawake-tray"]')).toBeVisible();
    await expect(titlebar.locator('[data-testid="peer-picker-button"]')).toHaveCount(0);
    await expect(
      page.locator('[data-testid="general-section"] [data-testid="peer-picker-button"]'),
      "peer picker is not in the sidebar GENERAL header",
    ).toBeVisible();

    // Viewport is locked: no pinch zoom, notch-aware, keyboard resizes content.
    const meta = await page.getAttribute('meta[name="viewport"]', "content");
    for (const directive of [
      "user-scalable=no",
      "maximum-scale=1.0",
      "viewport-fit=cover",
      "interactive-widget=resizes-content",
    ]) {
      expect(meta, `viewport meta missing ${directive}`).toContain(directive);
    }
  } finally {
    await page.context().close();
  }
});

test("@golden web mobile: hamburger reopens the dismissed sidebar overlay", async ({ api, webBrowser }) => {
  // A fresh instance holds only the protected General, which renders in
  // general-section and NEVER as a workspace-item card — so this test seeds the
  // ordinary workspace whose card it waits on, and takes it away again.
  const workspaceName = `pw-mobile-reopen-${Date.now()}`;
  await api.createWorkspace(workspaceName);
  const workspaceId = (await api.workspaces()).find((w) => w.name === workspaceName)?.id;
  expect(workspaceId, "sidebar fixture workspace was not created").toBeTruthy();

  const page = await newWebPage(webBrowser, { width: 375, height: 812 });
  try {
    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    // only way in, so its absence bricks mobile navigation.
    const burger = page.locator('[data-testid="web-sidebar-open"]');
    const workspaceCard = page.locator(`[data-testid="workspace-card"][data-workspace-id="${workspaceId}"]`);
    await expect(burger).toBeVisible({ timeout: 20_000 });
    await burger.click();
    await expect(page.locator('[data-testid="peer-picker-button"]')).toBeVisible({ timeout: 10_000 });
    await expect(
      workspaceCard.getByRole("button", { name: `Select workspace ${workspaceName}`, exact: true }),
      "seeded ordinary workspace did not render as a sidebar card",
    ).toBeVisible({ timeout: 10_000 });
    // Tapping the backdrop dismisses; the hamburger must come back.
    await page.mouse.click(370, 400);
    await expect(burger).toBeVisible({ timeout: 10_000 });
    await burger.click();
    await expect(page.locator(".ct-sidebar-drawer.is-open")).toBeVisible({ timeout: 10_000 });
  } finally {
    await page.context().close();
    if (workspaceId) await api.closeWorkspace(workspaceId).catch(() => {});
  }
});

// ── close-sync: actions propagate between web and desktop UIs ────────

test("@golden close sync: web close updates the desktop window, desktop close updates web", async ({ api, ui, webBrowser, tabCleanup }) => {
  test.setTimeout(180_000);

  const a = await tabCleanup.createTab<{
    id: string;
    tab_id: string;
  }>({ title: "pw-close-from-web" });
  const b = await tabCleanup.createTab<{
    id: string;
    tab_id: string;
  }>({ title: "pw-close-from-desktop" });
  const page = await newWebPage(webBrowser);
  try {
    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });

    const webTab = (id: string) => page.locator(`[data-testid="tab-item"][data-tab-id="${id}"]`);
    const deskTab = (id: string) => ui.page.locator(`[data-testid="tab-item"][data-tab-id="${id}"]`);
    for (const id of [a.tab_id, b.tab_id]) {
      await expect(webTab(id)).toBeVisible({ timeout: 20_000 });
      await expect(deskTab(id)).toBeVisible({ timeout: 20_000 });
    }

    // 1) Close A through the WEB sidebar UI (⋮ → Close Tab).
    await expect(webTab(a.tab_id), "web row for A before its menu").toBeVisible({ timeout: 15_000 });
    await webTab(a.tab_id).hover();
    const webActions = webTab(a.tab_id).locator('[title="Tab actions"]');
    await webActions.scrollIntoViewIfNeeded();
    await expect(webActions).toBeVisible({ timeout: 5_000 });
    await webActions.click();
    // The menu item carries `tab-menu-close` (TabItem.tsx). Its LABEL is
    // "Close Session" or "Close Tab" depending on whether the tab has an agent,
    // and the same words also appear on a CommandPalette entry — so a page-wide
    // text match could act on a different tab entirely.
    await page.getByTestId("tab-menu-close").click();
    await expect(webTab(a.tab_id)).toHaveCount(0, { timeout: 15_000 });
    // …and the DESKTOP window follows via mux-state-changed.
    await expect(deskTab(a.tab_id)).toHaveCount(0, { timeout: 15_000 });

    // 2) Close B through the DESKTOP sidebar UI.
    await expect(deskTab(b.tab_id), "desktop row for B before its menu").toBeVisible({ timeout: 15_000 });
    await deskTab(b.tab_id).hover();
    const deskActions = deskTab(b.tab_id).locator('[title="Tab actions"]');
    await deskActions.scrollIntoViewIfNeeded();
    await expect(deskActions).toBeVisible({ timeout: 5_000 });
    await deskActions.click();
    await ui.page.getByTestId("tab-menu-close").click();
    await expect(deskTab(b.tab_id)).toHaveCount(0, { timeout: 15_000 });
    // …and the WEB mirror follows via SSE.
    await expect(webTab(b.tab_id)).toHaveCount(0, { timeout: 15_000 });
  } finally {
    await api.closeTab(a.id).catch(() => {});
    await api.closeTab(b.id).catch(() => {});
    await page.context().close();
  }
});

// ── chat view in the browser — the mirror's core demo path ──────────

test("@stub web chat: an assistant reply streams in and history survives a reload", async ({ api, webBrowser, spawnAgent }) => {
  test.setTimeout(90_000);
  test.skip(
    !(await isStubTransportActive(api)),
    "requires CODETERM_STUB_AGENT_CMD; real providers are forbidden",
  );

  const spawned = await spawnAgent({
    agent: "claude",
    title: "pw-web-chat",
    skip_permissions: true,
    activate: true,
  });
  const tabId = spawned.tab_id;
  const page = await newWebPage(webBrowser);
  try {
    // until then chat-raw/start 409s.
    // chat-raw/start REQUIRES its subscriber id (ChatSubscriptionBody, camelCase);
    // a bodyless call 400s forever and the catch turns that into a readiness lie.
    const subscriptionId = `pw-web-chat-${tabId}`;
    await pollUntil(async () => {
      try {
        await api.post(`/tabs/${tabId}/chat-raw/start`, { subscriptionId });
        await api.post(`/tabs/${tabId}/chat-raw/stop`, { subscriptionId });
        return true;
      } catch {
        return undefined;
      }
    }, 120_000, 3_000, "spawned tab chat becomes ready");
    await api.post(`/tabs/${tabId}/chat-view`, { enabled: true });

    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 30_000 });

    // Scope everything to the spawned tab's subtree — other tabs (e.g.
    // the user's GA) may have chat-view mounted too.
    const tab = page.locator(`[data-tab-id="${tabId}"]`);
    const composer = tab.locator('[data-testid="chat-composer"]');
    await composer.waitFor({ timeout: 30_000 });

    // bubble that must stream back over SSE.
    const marker = `CHATMARKER-${Date.now()}`;
    await composer.fill(
      `Reply with a single line containing exactly ${marker} and nothing else.`,
    );
    // covered by desktop usage daily; the button only exists here).
    await tab.locator('[data-testid="composer-send-button"]').click();

    // The optimistic user bubble proves the send handler actually fired.
    await expect(
      tab.locator('[data-testid="chat-message"]').filter({ hasText: marker }).first(),
    ).toBeVisible({ timeout: 15_000 });

    const reply = tab
      .locator('[data-testid="chat-message"][data-message-role="assistant"]')
      .filter({ hasText: marker });
    const sendError = tab.locator('[data-testid="chat-composer-error"]');
    await expect(reply.or(sendError).first()).toBeVisible({ timeout: 30_000 });
    if (await sendError.isVisible()) {
      throw new Error(`composer send failed: ${await sendError.textContent()}`);
    }
    await expect(reply).toBeVisible();

    // re-render purely from the chat-raw JSONL window (was: empty chat).
    await page.reload({ waitUntil: "domcontentloaded" });
    await composer.waitFor({ timeout: 30_000 });
    await expect(
      tab
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: marker }),
    ).toBeVisible({ timeout: 30_000 });

    // shows up as a chip with the SERVER-side filename.
    await tab
      .locator('[data-testid="composer-attach-input"]')
      .setInputFiles({
        name: "pw-attach.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("attach-pipeline-proof"),
      });
    const chip = tab.locator('[data-testid="attachment-chip"]');
    await expect(chip).toBeVisible({ timeout: 15_000 });
    await expect(chip).toContainText("pw-attach.txt");
  } finally {
    await api.closeTab(tabId).catch(() => {});
    await page.context().close();
  }
});
