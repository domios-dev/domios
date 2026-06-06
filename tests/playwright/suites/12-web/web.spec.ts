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
  // The peer picker lives in the titlebar — visible on every viewport
  // (workspace items hide inside the collapsed mobile sidebar).
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
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

    // Switch: reload + per-peer token gate for the stub host.
    const before = stub.requests.length;
    await item.click();
    await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 20_000 });
    await page.fill('[data-testid="web-token-input"]', STUB_TOKEN);
    await page.click('[data-testid="web-token-submit"]');

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

  // Sidebar starts collapsed on phones — workspace items live in the
  // overlay, which the hamburger opens.
  // (passTokenGate waited for workspace-item: on mobile it may be in the
  // initially-closed sidebar; tolerate either, then exercise the toggle.)
  const picker = page.locator('[data-testid="peer-picker-button"]');
  await expect(picker).toBeVisible();
  const box = await picker.boundingBox();
  expect(box && box.height >= 40, `picker touch target ${box?.height}px`).toBe(true);

  // Mesh tab is reachable: open sidebar if collapsed, then nav-mesh.
  const navMesh = page.locator('[data-testid="nav-mesh"]');
  if (!(await navMesh.isVisible())) {
    await page.locator('button[title="Toggle sidebar"]').click();
  }
  await expect(navMesh).toBeVisible({ timeout: 5_000 });
  await navMesh.click();
  await expect(page.locator('[data-testid="mesh-tab-network"]')).toBeVisible({ timeout: 10_000 });
  await page.context().close();
});

test("@golden fully live on mobile: General Agent responds, read from rendered terminal DOM", async () => {
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

    await page.waitForFunction(
      (m) =>
        Array.from(document.querySelectorAll(".xterm-accessibility")).some((tree) =>
          (tree.textContent ?? "").includes(m),
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
