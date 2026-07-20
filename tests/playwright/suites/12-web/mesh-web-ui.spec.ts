/**
 * WHAT: the global Mesh header "Web UI" control and the per-peer "Open Web UI"
 * action cover their Playwright acceptance contract.
 * WHY: protects the frontend projection of the backend reconciliation snapshot
 * (task_x7jps4) from regressing into inferred state, a canvas-only label, or a
 * responsive layout that overlaps at narrow widths.
 * ORACLE: exact data-testid/data-web-ui-state selectors and action
 * availability per backend snapshot fixture, run at desktop and mobile
 * viewports.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

let token: string;

test.beforeAll(async () => {
  token = readDevToken();
});

type Json = Record<string, unknown>;

interface SnapshotFixture {
  state: "ready" | "setup_required" | "operator_bootstrap_required" | "conflict" | "unhealthy";
  overrides?: Json;
}

async function captureOpenedUrl(page: Page): Promise<void> {
  await page.evaluate(() => {
    const target = window as Window & { __ctOpenedUrl?: string };
    window.open = ((url?: string | URL) => {
      target.__ctOpenedUrl = String(url ?? "");
      return null;
    }) as typeof window.open;
  });
}

async function openedUrl(page: Page): Promise<string | undefined> {
  return page.evaluate(() => (window as Window & { __ctOpenedUrl?: string }).__ctOpenedUrl);
}

function buildSnapshot({ state, overrides = {} }: SnapshotFixture): Json {
  const base: Json = {
    state,
    url: state === "ready" ? "https://blackstar.test.ts.net/" : null,
    api_port: 7685,
    desired_https_port: 443,
    chosen_https_port: state === "ready" ? 443 : null,
    operator_bootstrap_required: state === "operator_bootstrap_required",
    bootstrap_command:
      state === "operator_bootstrap_required" ? "sudo tailscale set --operator=rollacode" : null,
    binary: {
      installed: state !== "setup_required",
      path: "/usr/bin/tailscale",
      cli_version: "1.80.0",
      daemon_version: "1.80.0",
      version_skew: false,
      version_output: null,
      error: null,
    },
    route:
      state === "ready"
        ? { host: "blackstar.test.ts.net", https_port: 443, proxy: "http://127.0.0.1:7685", ownership: "owned" }
        : null,
    routes:
      state === "ready"
        ? [{ host: "blackstar.test.ts.net", https_port: 443, proxy: "http://127.0.0.1:7685", ownership: "owned" }]
        : [],
    conflict: state === "conflict" ? "Port 443 already proxies http://127.0.0.1:9999." : null,
    health: {
      checked_url: state === "ready" ? "https://blackstar.test.ts.net/api/health" : null,
      ok: state === "ready",
      status: state === "unhealthy" ? 502 : state === "ready" ? 200 : null,
      daemon_build: "abc123",
      expected_build: "abc123",
      build_matches: state === "ready" ? true : null,
      error: state === "unhealthy" ? "connection refused" : null,
    },
  };
  return { ...base, ...overrides };
}

function pairedPeerFixture(over: Json = {}): Json {
  return {
    instance_id: "peer-blackstar",
    instance_name: "blackstar",
    addresses: [{ ip: "100.64.0.9", is_tailscale: true }],
    api_port: 7685,
    public_key: "pk-test",
    paired_at: "2026-01-01T00:00:00Z",
    last_seen: "2026-07-11T00:00:00Z",
    online: true,
    is_primary: false,
    access_level: "full",
    version: "1.7.2",
    https_url: null,
    ...over,
  };
}

async function mockMeshBackend(
  page: Page,
  opts: { snapshot: Json | null; loading?: boolean; peers?: Json[] },
): Promise<() => void> {
  let releaseLoading = () => {};
  const loadingGate = opts.loading
    ? new Promise<void>((resolve) => { releaseLoading = resolve; })
    : null;
  await page.route("**/api/network", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ local_ip: "192.168.1.20", api_port: 7685 }),
    }),
  );
  await page.route("**/api/network/serve/status", async (route) => {
    if (loadingGate) {
      await loadingGate;
      return route.fulfill({ status: 504, body: "" });
    }
    return route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        installed: (opts.snapshot?.binary as Json | undefined)?.installed ?? false,
        active: opts.snapshot?.state === "ready",
        https_url: (opts.snapshot?.url as string | null) ?? null,
        platform: "linux",
        snapshot: opts.snapshot,
      }),
    });
  });
  await page.route("**/api/network/serve/enable", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify({ ok: true, code: "enabled", warning: null }) }),
  );
  await page.route("**/api/network/serve/disable", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify({ ok: true, code: "disabled" }) }),
  );
  await page.route("**/api/mesh/status", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        enabled: true,
        instance_id: "self",
        instance_name: "blackstar",
        discovery_active: true,
        broadcast_active: true,
        broadcast_expires_at: null,
        tailscale_ip: "100.64.0.9",
        local_ip: "192.168.1.20",
        api_port: 7685,
        paired_count: opts.peers?.length ?? 0,
        discovered_count: 0,
      }),
    }),
  );
  await page.route("**/api/mesh/peers", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ paired: opts.peers ?? [], discovered: [], incoming_requests: [] }),
    }),
  );
  await page.route("**/api/mesh/peers/**/services", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ services: [], stale: false, fetched_at: Date.now(), peer_online: true, error: null }),
    }),
  );
  await page.route("**/api/services", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify([]) }),
  );
  return releaseLoading;
}

async function gotoMesh(
  browser: Browser,
  opts: { snapshot: Json | null; loading?: boolean; peers?: Json[] },
  viewport: { width: number; height: number } = { width: 1280, height: 800 },
): Promise<{ page: Page; close: () => Promise<void> }> {
  const ctx = await browser.newContext({ viewport, serviceWorkers: "block" });
  await ctx.route("https://*.test.ts.net/**", (route) =>
    route.fulfill({ contentType: "text/html", body: "<!doctype html><title>Web UI fixture</title>" }),
  );
  const page = await ctx.newPage();
  const releaseLoading = await mockMeshBackend(page, opts);

  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 15_000 });
  await page.fill('[data-testid="web-token-input"]', token);
  await page.click('[data-testid="web-token-submit"]');
  await page
    .locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]')
    .first()
    .waitFor({ timeout: 20_000 });

  const sidebarOpen = page.locator('[data-testid="web-sidebar-open"]');
  if (await sidebarOpen.isVisible().catch(() => false)) await sidebarOpen.click();
  await page.click('[data-testid="nav-mesh"]');
  await page.waitForSelector('[data-testid="mesh-web-ui-action"]', { timeout: 15_000 });
  return {
    page,
    close: async () => {
      releaseLoading();
      await ctx.close();
    },
  };
}

const WEB_URL = `${API_BASE}/`;

const STATES: SnapshotFixture["state"][] = [
  "ready",
  "setup_required",
  "operator_bootstrap_required",
  "conflict",
  "unhealthy",
];

for (const state of STATES) {
  test(`@golden mesh-web-ui: header projects backend state "${state}" with exact action set`, async ({ webBrowser }) => {
    const { page, close } = await gotoMesh(webBrowser, { snapshot: buildSnapshot({ state }) });
    try {
      const action = page.locator('[data-testid="mesh-web-ui-action"]');
      await expect(action).toHaveAttribute("data-web-ui-state", state);

      await expect(page.locator('[data-testid="mesh-web-ui-surface"]')).toBeVisible();
      await expect(page.locator('[data-testid="mesh-web-ui-menu-trigger"]')).toBeVisible();
      await expect(page.locator('[data-testid="mesh-web-ui-open"]')).toHaveCount(0);
    } finally {
      await close();
    }
  });
}

test("@golden mesh-web-ui: loading shows Checking chip before the first snapshot resolves", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, { snapshot: null, loading: true });
  try {
    const action = page.locator('[data-testid="mesh-web-ui-action"]');
    await expect(action).toHaveAttribute("data-web-ui-state", "loading");
    await expect(page.locator('[data-testid="mesh-web-ui-open"]')).toHaveCount(0);
    await expect(page.locator('[data-testid="mesh-web-ui-menu-trigger"]')).toHaveCount(0);
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: no tiny canvas-only Web UI label remains", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, {
    snapshot: buildSnapshot({ state: "ready" }),
    peers: [pairedPeerFixture({ https_url: "https://peer.test.ts.net/" })],
  });
  try {
    await expect(page.locator('[data-testid="web-ui-trigger"]')).toHaveCount(0);
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: setup action calls the backend and shows an inline message, not a toast", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, { snapshot: buildSnapshot({ state: "setup_required" }) });
  try {
    await page.click('[data-testid="mesh-web-ui-menu-trigger"]');
    await page.waitForSelector('[data-testid="mesh-web-ui-menu"]');
    await page.click('[data-testid="mesh-web-ui-setup"]');
    await expect(page.locator('[data-testid="mesh-web-ui-message"]')).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('.toast, [role="status"][data-toast]')).toHaveCount(0);
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: repair action on conflict reconciles and clears to a truthful message", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, { snapshot: buildSnapshot({ state: "conflict" }) });
  try {
    await page.click('[data-testid="mesh-web-ui-menu-trigger"]');
    await page.click('[data-testid="mesh-web-ui-repair"]');
    await expect(page.locator('[data-testid="mesh-web-ui-message"]')).toBeVisible({ timeout: 10_000 });
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: ready surface opens the access dialog and launches its generated URL", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, { snapshot: buildSnapshot({ state: "ready" }) });
  try {
    await captureOpenedUrl(page);
    await page.click('[data-testid="mesh-web-ui-surface"]');
    const dialog = page.getByRole("dialog", { name: "Web UI access" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Open" }).click();
    expect(await openedUrl(page)).toBe(`http://192.168.1.20:7685/#token=${token}`);
  } finally {
    await close();
  }
});

test("@golden peer-web-ui-open: appears only for a peer with a verified URL", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, {
    snapshot: buildSnapshot({ state: "ready" }),
    peers: [
      pairedPeerFixture({ instance_id: "peer-verified", instance_name: "verified-peer", https_url: "https://verified-peer.test.ts.net/" }),
      pairedPeerFixture({ instance_id: "peer-unverified", instance_name: "unverified-peer", https_url: null }),
    ],
  });
  try {
    await captureOpenedUrl(page);
    await expect(page.locator('[data-testid="peer-web-ui-open"]')).toHaveCount(1);
    const btn = page.locator('[data-testid="peer-web-ui-open"]');
    await expect(btn).toHaveAttribute("data-peer-web-url", "https://verified-peer.test.ts.net/");

    await btn.click();
    expect(await openedUrl(page)).toBe("https://verified-peer.test.ts.net/");
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: desktop and mobile header selectors do not overlap", async ({ webBrowser }) => {
  for (const viewport of [{ width: 1280, height: 800 }, { width: 375, height: 812 }]) {
    const { page, close } = await gotoMesh(
      webBrowser,
      { snapshot: buildSnapshot({ state: "ready" }) },
      viewport,
    );
    try {
      const webUiBox = await page.locator('[data-testid="mesh-web-ui-action"]').boundingBox();
      const tabsBox = await page.locator('[data-testid="mesh-tab-map"]').boundingBox();
      expect(webUiBox, `web ui control visible at ${viewport.width}px`).toBeTruthy();
      expect(tabsBox, `tabs visible at ${viewport.width}px`).toBeTruthy();
      if (webUiBox && tabsBox) {
        const noOverlap = webUiBox.x + webUiBox.width <= tabsBox.x || tabsBox.x + tabsBox.width <= webUiBox.x;
        expect(noOverlap, `mesh-web-ui-action and mesh-tab-map must not overlap at ${viewport.width}px`).toBe(true);
      }
    } finally {
      await close();
    }
  }
});
