/**
 * 12-web — the web mirror in a REAL BROWSER (launched msedge, not the
 * Tauri WebView). Token gate, peer switching against a scripted stub
 * peer, mobile viewport, and a fully live LLM round-trip read from the
 * rendered terminal DOM (/web/?a11y=1 runtime opt-in).
 */
import { chromium, expect, test, type Browser, type Page } from "@playwright/test";
import { ApiClient, readDevToken } from "../../helpers/api-client";
import { API_BASE } from "../../helpers/dev-console";
import { MeshStub } from "../../helpers/mesh-stub";

const WEB_URL = `${API_BASE}/web/`;
const api = new ApiClient();
let browser: Browser;

test.beforeAll(async () => {
  browser = await chromium.launch({ channel: "msedge" });
});

test.afterAll(async () => {
  await browser.close();
});

async function newWebPage(viewport?: { width: number; height: number }): Promise<Page> {
  const ctx = await browser.newContext({ viewport: viewport ?? { width: 1280, height: 800 } });
  return ctx.newPage();
}

async function passTokenGate(page: Page): Promise<void> {
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 15_000 });
  await page.fill('[data-testid="web-token-input"]', readDevToken());
  await page.click('[data-testid="web-token-submit"]');
  // Past the gate: desktop boots with the sidebar open (peer picker in its
  // web top block); phones boot collapsed and show the floating hamburger.
  await page
    .locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]')
    .first()
    .waitFor({ timeout: 20_000 });
}

test("@smoke /web serves the SPA; token gate authenticates; self is the default host", async () => {
  const page = await newWebPage();
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await passTokenGate(page);

  // Default target is the origin that served the bundle.
  const host = await page.evaluate(() => sessionStorage.getItem("codeterm-web-host"));
  expect(host).toBeNull();

  // The peer picker is mounted top-left and shows "this host".
  await expect(page.locator('[data-testid="peer-picker-button"]')).toBeVisible();
  await expect(page.locator('[data-testid="peer-picker-button"]')).toContainText("this host");
  await page.context().close();
});

test("@golden bad token is rejected with a rendered error, not a broken app", async () => {
  const page = await newWebPage();
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]');
  await page.fill('[data-testid="web-token-input"]', "definitely-wrong-token");
  await page.click('[data-testid="web-token-submit"]');
  await expect(page.locator('[data-testid="web-token-error"]')).toBeVisible({ timeout: 10_000 });
  await page.context().close();
});

test("@golden peer switching: dropdown lists the stub, traffic routes to it, switch-back restores", async () => {
  test.setTimeout(240_000);
  const STUB_TOKEN = "stub-web-token-123";
  const stub = new MeshStub({ services: 1, webToken: STUB_TOKEN });
  let peerId: string | undefined;

  // Stale stub peers persist in peers.json when an earlier run crashed
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
    // Seed the stub with this daemon's real config so the SPA's boot path
    // has every field it expects when pointed at the stub.
    stub.configPayload = await api.get("/config");
    await stub.start("happy");
    const added = await api.post<{ instance_id: string }>("/mesh/peers/add-manual", {
      address: "127.0.0.1",
      port: stub.port,
    });
    peerId = added.instance_id;
    stub.instanceId = peerId;
    await api.post("/mesh/connect", { instance_id: peerId });
    const deadline = Date.now() + 20_000;
    for (;;) {
      const peers = await api.get<{ paired: { instance_id: string }[] }>("/mesh/peers");
      if (peers.paired.some((p) => p.instance_id === peerId)) break;
      if (Date.now() > deadline) throw new Error("stub never paired");
      await new Promise((r) => setTimeout(r, 500));
    }

    const page = await newWebPage();
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    await passTokenGate(page);

    // Dropdown lists the stub with its online state.
    await page.click('[data-testid="peer-picker-button"]');
    const item = page.locator(`[data-testid="peer-picker-item"][data-peer-name="${stub.instanceName}"]`);
    await expect(item).toBeVisible({ timeout: 15_000 });
    await expect(item).toHaveAttribute("data-peer-online", "true");

    // Switch: the current daemon mints the stub's web token over mesh,
    // stores it for the target host, then reloads directly into the stub.
    const before = stub.requests.length;
    await item.click();
    await expect(page.locator('[data-testid="web-token-input"]')).toHaveCount(0);

    // PROOF of routing: the stub's workspace renders in the sidebar and
    // the stub actually received the web client's API calls.
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${stub.instanceName}-workspace"]`),
    ).toBeVisible({ timeout: 20_000 });
    expect(stub.requests.length, "stub saw no web traffic after switch").toBeGreaterThan(before);
    expect(
      stub.requests.some((r) => r.includes("/api/mux/state") || r.includes("/api/workspaces")),
      `stub requests: ${stub.requests.slice(-10).join(", ")}`,
    ).toBe(true);
    expect(
      stub.requests.some((r) => r.includes("/api/mesh/web-token")),
      `stub requests: ${stub.requests.slice(-10).join(", ")}`,
    ).toBe(true);

    // Switch back to self: real workspaces return, no stub data leaks.
    await page.click('[data-testid="peer-picker-button"]');
    await page.click('[data-testid="peer-picker-self"]');
    await page.waitForSelector('[data-testid="workspace-item"]', { timeout: 20_000 });
    await expect(
      page.locator(`[data-testid="workspace-item"][data-workspace-name="${stub.instanceName}-workspace"]`),
    ).toHaveCount(0);
    const host = await page.evaluate(() => sessionStorage.getItem("codeterm-web-host"));
    expect(host, "switch-back must clear the host override").toBeNull();
    await page.context().close();
  } finally {
    if (peerId) await api.post(`/mesh/peers/disconnect/${peerId}`).catch(() => {});
    await stub.stop();
  }
});

test("@golden unreachable peer fails gracefully at the gate, back-to-self recovers", async () => {
  test.setTimeout(120_000);
  const page = await newWebPage();
  // Point the app at a dead host directly (sessionStorage), as if a peer
  // went down after being selected.
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await page.evaluate(() => {
    sessionStorage.setItem("codeterm-web-host", "http://127.0.0.1:39999");
    localStorage.setItem("codeterm-web-token:http://127.0.0.1:39999", "x");
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  // Gate must surface the failure and offer the way home.
  await page.waitForSelector('[data-testid="web-token-back-to-self"]', { timeout: 30_000 });
  await page.click('[data-testid="web-token-back-to-self"]');
  await passTokenGate(page);
  await page.context().close();
});

test("@golden mobile viewport: no horizontal scroll, sidebar overlays, picker reachable", async () => {
  const page = await newWebPage({ width: 375, height: 812 });
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await passTokenGate(page);

  const scrollW = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(scrollW, "horizontal scroll on phone viewport").toBeLessThanOrEqual(375);

  // Sidebar starts collapsed on phones — everything (peer picker,
  // workspaces, nav) lives in the overlay the hamburger opens.
  const burger = page.locator('[data-testid="web-sidebar-open"]');
  if (await burger.isVisible()) {
    await burger.click();
  }
  const picker = page.locator('[data-testid="peer-picker-button"]');
  await expect(picker).toBeVisible({ timeout: 10_000 });
  const box = await picker.boundingBox();
  expect(box && box.height >= 40, `picker touch target ${box?.height}px`).toBe(true);

  const navMesh = page.locator('[data-testid="nav-mesh"]');
  await expect(navMesh).toBeVisible({ timeout: 5_000 });
  await navMesh.click();
  await expect(page.locator('[data-testid="mesh-tab-network"]')).toBeVisible({ timeout: 10_000 });
  await page.context().close();
});

test("@golden fully live on mobile: General Agent responds, read from the rendered DOM", async () => {
  test.setTimeout(300_000);
  const page = await newWebPage({ width: 375, height: 812 });
  // Runtime a11y opt-in gives the browser terminal a readable DOM.
  await page.goto(`${WEB_URL}?a11y=1`, { waitUntil: "domcontentloaded" });
  await passTokenGate(page);

  const marker = `PW-WEB-${Date.now()}`;
  // Drive the spawn through the API (deterministic), observe in the DOM.
  const inbox = (await api.createPane({ title: "pw-web-inbox" })) as { id: string };
  let agentPaneId: string | undefined;
  try {
    await api.post(`/panes/${inbox.id}/role`, { role: "Orchestrator" });
    const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
      agent: "claude",
      model: "sonnet",
      role: "Worker",
      title: "pw-web-agent",
      parent_pane_id: inbox.id,
      report_target_pane_id: inbox.id,
      skip_permissions: true,
      activate: true,
      task: `Reply with a single line containing exactly ${marker} and nothing else.`,
    });
    agentPaneId = spawned.pane_id;

    // Mobile defaults supported agent panes to chat mode, so the reply may
    // render as a chat bubble instead of xterm a11y text — accept either.
    await page.waitForFunction(
      (m) =>
        Array.from(document.querySelectorAll(".xterm-accessibility")).some((tree) =>
          (tree.textContent ?? "").includes(m),
        ) ||
        Array.from(document.querySelectorAll('[data-testid="chat-message"]')).some(
          (bubble) => (bubble.textContent ?? "").includes(m),
        ),
      marker,
      { timeout: 240_000, polling: 1_000 },
    );
  } finally {
    if (agentPaneId) await api.closePane(agentPaneId).catch(() => {});
    await api.closePane(inbox.id).catch(() => {});
    await page.context().close();
  }
});

// ── share-link token auto-consume ────────────────────────────────────

test("@golden share link: #token= auto-enters and the URL is redacted", async () => {
  const page = await newWebPage();
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  // No manual gate — straight to the app.
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
  const url = page.url();
  expect(url, "token leaked in the URL after consume").not.toContain("token=");
  await page.context().close();
});

test("@golden share link: ?token= query form also works and is redacted", async () => {
  const page = await newWebPage();
  await page.goto(`${WEB_URL}?token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
  expect(page.url()).not.toContain("token=");
  await page.context().close();
});

test("@golden share link: a bad token falls through to the gate error path", async () => {
  const page = await newWebPage();
  await page.goto(`${WEB_URL}#token=definitely-bad-token`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 20_000 });
  await expect(page.locator('[data-testid="web-token-error"]')).toBeVisible({ timeout: 10_000 });
  expect(page.url()).not.toContain("token=");
  await page.context().close();
});

// ── web chrome strip + mobile viewport hardening ─────────────────────

test("@golden web chrome: titlebar absent, sidebar hosts peer picker + keep-awake", async () => {
  const page = await newWebPage();
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });

  // The desktop titlebar row must NOT render in the browser (01-smoke
  // asserts the inverse for the Tauri window).
  await expect(page.locator('[data-testid="titlebar"]')).toHaveCount(0);
  // Its tenants moved into the sidebar's web-only top block.
  await expect(page.locator('[data-testid="keepawake-tray"]')).toBeVisible();

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
  await page.context().close();
});

test("@golden web mobile: hamburger reopens the dismissed sidebar overlay", async () => {
  const page = await newWebPage({ width: 375, height: 812 });
  await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  // Phones boot with the sidebar collapsed — the floating hamburger is the
  // only way in, so its absence bricks mobile navigation.
  const burger = page.locator('[data-testid="web-sidebar-open"]');
  await expect(burger).toBeVisible({ timeout: 20_000 });
  await burger.click();
  await expect(page.locator('[data-testid="peer-picker-button"]')).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('[data-testid="workspace-item"]').first()).toBeVisible({ timeout: 10_000 });
  // Tapping the backdrop dismisses; the hamburger must come back.
  await page.mouse.click(370, 400);
  await expect(burger).toBeVisible({ timeout: 10_000 });
  await page.context().close();
});

// ── close-sync: actions propagate between web and desktop UIs ────────

test("@golden close sync: web close updates the desktop window, desktop close updates web", async () => {
  test.setTimeout(180_000);
  const { connectUi } = await import("../../helpers/dev-console");

  const a = (await api.createPane({ title: "pw-close-from-web" })) as {
    id: string;
    tab_id: string;
  };
  const b = (await api.createPane({ title: "pw-close-from-desktop" })) as {
    id: string;
    tab_id: string;
  };
  const ui = await connectUi();
  const page = await newWebPage();
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
    await webTab(a.tab_id).hover();
    await webTab(a.tab_id).locator('[title="Tab actions"]').click({ force: true });
    await page.getByText(/^Close (Tab|Session)$/).first().click();
    await expect(webTab(a.tab_id)).toHaveCount(0, { timeout: 15_000 });
    // …and the DESKTOP window follows via mux-state-changed.
    await expect(deskTab(a.tab_id)).toHaveCount(0, { timeout: 15_000 });

    // 2) Close B through the DESKTOP sidebar UI.
    await deskTab(b.tab_id).hover();
    await deskTab(b.tab_id).locator('[title="Tab actions"]').click({ force: true });
    await ui.page.getByText(/^Close (Tab|Session)$/).first().click();
    await expect(deskTab(b.tab_id)).toHaveCount(0, { timeout: 15_000 });
    // …and the WEB mirror follows via SSE.
    await expect(webTab(b.tab_id)).toHaveCount(0, { timeout: 15_000 });
  } finally {
    await api.closePane(a.id).catch(() => {});
    await api.closePane(b.id).catch(() => {});
    await ui.browser.close().catch(() => {});
    await page.context().close();
  }
});

// ── chat view in the browser — the mirror's core demo path ──────────

test("@golden web chat: a live assistant reply streams in AND history survives a reload", async () => {
  // Regression for the 'No messages yet' bug: chat_start in web returned
  // the wrong shape (parsed snapshot instead of raw JSONL), and assistant
  // deltas (pane-chat-raw-{id}) were never mirrored to SSE.
  //
  // Hermetic by design: spawns its OWN claude pane. The GA pane belongs to
  // the user — it may be mid-task (a marker prompt then queues behind real
  // work and times out), and toggling its chat-view flips the desktop UI.
  test.setTimeout(300_000);

  const spawned = await api.post<{ pane_id: string }>("/agents/spawn", {
    agent: "claude",
    model: "sonnet",
    title: "pw-web-chat",
    skip_permissions: true,
    activate: true,
  });
  const paneId = spawned.pane_id;
  const page = await newWebPage();
  try {
    // The chat source is ready once the claude CLI has booted a session —
    // until then chat-raw/start 409s.
    const ready = Date.now() + 120_000;
    for (;;) {
      try {
        await api.post(`/panes/${paneId}/chat-raw/start`);
        await api.post(`/panes/${paneId}/chat-raw/stop`);
        break;
      } catch (e) {
        if (Date.now() > ready) throw new Error(`spawned pane chat never became ready: ${e}`);
        await new Promise((r) => setTimeout(r, 3_000));
      }
    }
    await api.post(`/panes/${paneId}/chat-view`, { enabled: true });

    await page.goto(`${WEB_URL}#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
    await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 30_000 });

    // Scope everything to the spawned pane's subtree — other panes (e.g.
    // the user's GA) may have chat-view mounted too.
    const pane = page.locator(`[data-pane-id="${paneId}"]`);
    const composer = pane.locator('[data-testid="chat-composer"]');
    await composer.waitFor({ timeout: 30_000 });

    // Live round-trip: send through the real composer, read the assistant
    // bubble that must stream back over SSE.
    const marker = `CHATMARKER-${Date.now()}`;
    await composer.fill(
      `Reply with a single line containing exactly ${marker} and nothing else.`,
    );
    // Send via the explicit button — the touch-screen path (Enter is
    // covered by desktop usage daily; the button only exists here).
    await pane.locator('[data-testid="composer-send-button"]').click();

    // The optimistic user bubble proves the send handler actually fired.
    await expect(
      pane.locator('[data-testid="chat-message"]').filter({ hasText: marker }).first(),
    ).toBeVisible({ timeout: 15_000 });

    const reply = pane
      .locator('[data-testid="chat-message"][data-message-role="assistant"]')
      .filter({ hasText: marker });
    const sendError = pane.locator('[data-testid="chat-composer-error"]');
    await expect(reply.or(sendError).first()).toBeVisible({ timeout: 240_000 });
    if (await sendError.isVisible()) {
      throw new Error(`composer send failed: ${await sendError.textContent()}`);
    }
    await expect(reply).toBeVisible();

    // History: reload drops all live SSE state — the same bubbles must
    // re-render purely from the chat-raw JSONL window (was: empty chat).
    await page.reload({ waitUntil: "domcontentloaded" });
    await composer.waitFor({ timeout: 30_000 });
    await expect(
      pane
        .locator('[data-testid="chat-message"][data-message-role="assistant"]')
        .filter({ hasText: marker }),
    ).toBeVisible({ timeout: 30_000 });

    // Attach pipeline (no LLM): a browser file uploads to the daemon and
    // shows up as a chip with the SERVER-side filename.
    await pane
      .locator('[data-testid="composer-attach-input"]')
      .setInputFiles({
        name: "pw-attach.txt",
        mimeType: "text/plain",
        buffer: Buffer.from("attach-pipeline-proof"),
      });
    const chip = pane.locator('[data-testid="attachment-chip"]');
    await expect(chip).toBeVisible({ timeout: 15_000 });
    await expect(chip).toContainText("pw-attach.txt");
  } finally {
    await api.closePane(paneId).catch(() => {});
    await page.context().close();
  }
});
