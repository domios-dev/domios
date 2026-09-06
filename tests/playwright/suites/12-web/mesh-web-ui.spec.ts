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
import { RELAY_PROTOCOL_VERSION } from "../../../../src/lib/types";

let token: string;

/** The Serve URL every `ready` fixture publishes. Both the header control's access
 *  link and each peer's relay link are derived from THIS host's snapshot url, so
 *  the expectations below read it from here instead of restating the literal. */
const READY_URL = "https://blackstar.test.ts.net/";

/** Every affordance MeshWebUiControl can render, all of them inside the menu. */
const MENU_ACTIONS = [
  "mesh-web-ui-open",
  "mesh-web-ui-setup",
  "mesh-web-ui-repair",
  "mesh-web-ui-copy-url",
  "mesh-web-ui-surface",
  "mesh-web-ui-disable",
] as const;

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
    url: state === "ready" ? READY_URL : null,
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
    // MeshCanvas gates a peer's Web UI link on `online` AND a matching relay
    // protocol version, then builds the link from THIS host's Serve url — the
    relay_protocol_version: RELAY_PROTOCOL_VERSION,
    ...over,
  };
}

/** The link MeshCanvas hands a peer card: this host's Serve url, our token, and
 *  the peer as the relay target (webLinkWithTokenAndRelay). */
function peerRelayUrl(instanceId: string): string {
  return `${READY_URL}#token=${encodeURIComponent(token)}&relay=${instanceId}`;
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

/**
 * Read straight off `deriveWebUiView` (src/components/Titlebar/meshWebUi.ts), the
 * single place the control's affordances are decided: `url` gates open + copy,
 * `ownsRoute` gates disable, and the state gates setup vs repair. Only `ready`
 * publishes a url and an owned route in these fixtures, so only `ready` offers
 * open/copy/disable. The QR surface is unconditional.
 */
const EXPECTED_ACTIONS: Record<SnapshotFixture["state"], readonly string[]> = {
  ready: ["mesh-web-ui-open", "mesh-web-ui-copy-url", "mesh-web-ui-surface", "mesh-web-ui-disable"],
  setup_required: ["mesh-web-ui-setup", "mesh-web-ui-surface"],
  operator_bootstrap_required: ["mesh-web-ui-setup", "mesh-web-ui-surface"],
  conflict: ["mesh-web-ui-repair", "mesh-web-ui-surface"],
  unhealthy: ["mesh-web-ui-repair", "mesh-web-ui-surface"],
};

async function expectMenuOffers(page: Page, expected: readonly string[], label: string): Promise<void> {
  for (const testid of MENU_ACTIONS) {
    const wanted = expected.includes(testid);
    await expect(
      page.locator(`[data-testid="${testid}"]`),
      `${label}: ${testid} must ${wanted ? "be offered" : "not be offered"}`,
    ).toHaveCount(wanted ? 1 : 0);
  }
}

for (const state of STATES) {
  test(`@golden mesh-web-ui: header projects backend state "${state}" with exact action set`, async ({ webBrowser }) => {
    const { page, close } = await gotoMesh(webBrowser, { snapshot: buildSnapshot({ state }) });
    try {
      const action = page.locator('[data-testid="mesh-web-ui-action"]');
      await expect(action).toHaveAttribute("data-web-ui-state", state);

      // The header exposes exactly ONE control; every affordance lives behind it,
      // so none of them may leak into the header while the menu is shut.
      const trigger = page.locator('[data-testid="mesh-web-ui-menu-trigger"]');
      await expect(trigger).toBeVisible();
      await expectMenuOffers(page, [], `${state} menu closed`);

      await trigger.click();
      await expect(page.locator('[data-testid="mesh-web-ui-menu"]')).toBeVisible();
      await expectMenuOffers(page, EXPECTED_ACTIONS[state], `${state} menu open`);
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
    await expect(page.locator('[data-testid="mesh-web-ui-chip"]')).toContainText("Checking…");

    // With no snapshot yet, deriveWebUiView grants no capability at all: the menu
    // opens onto the QR surface and nothing that could act on unknown state.
    await page.locator('[data-testid="mesh-web-ui-menu-trigger"]').click();
    await expect(page.locator('[data-testid="mesh-web-ui-menu"]')).toBeVisible();
    await expectMenuOffers(page, ["mesh-web-ui-surface"], "loading menu open");
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: no tiny canvas-only Web UI label remains", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, {
    snapshot: buildSnapshot({ state: "ready" }),
    peers: [pairedPeerFixture()],
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
    await page.click('[data-testid="mesh-web-ui-menu-trigger"]');
    await page.click('[data-testid="mesh-web-ui-surface"]');
    const dialog = page.getByRole("dialog", { name: "Web UI access" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Open" }).click();
    expect(await openedUrl(page)).toBe(`http://192.168.1.20:7685/#token=${token}`);
  } finally {
    await close();
  }
});

test("@golden peer-web-ui-open: enabled only for a peer that can actually be relayed to", async ({ webBrowser }) => {
  const { page, close } = await gotoMesh(webBrowser, {
    snapshot: buildSnapshot({ state: "ready" }),
    peers: [
      pairedPeerFixture({ instance_id: "peer-verified", instance_name: "verified-peer" }),
      // Same peer, one version behind: MeshCanvas refuses to hand it a relay link.
      pairedPeerFixture({
        instance_id: "peer-stale",
        instance_name: "stale-peer",
        relay_protocol_version: RELAY_PROTOCOL_VERSION - 1,
      }),
    ],
  });
  try {
    await captureOpenedUrl(page);
    const button = (relayTarget: string) =>
      page.locator(`[data-testid="peer-web-ui-open"][data-peer-web-url="${peerRelayUrl(relayTarget)}"]`);

    // Every card carries the control; only a reachable peer on the current relay
    // protocol gets a URL, and only then is the control enabled.
    const verified = button("peer-verified");
    await expect(verified).toBeEnabled();
    await expect(button("self"), "this machine's own card links to itself").toBeEnabled();
    const stale = page.locator('[data-testid="peer-web-ui-open"]:not([data-peer-web-url])');
    await expect(stale, "a peer that cannot be relayed to must not offer a URL").toHaveCount(1);
    await expect(stale).toBeDisabled();

    await verified.click();
    // The link is THIS host's Web UI with the peer as the relay target — not the
    // peer's own address (webLinkWithTokenAndRelay in src/lib/webAccess.ts).
    expect(await openedUrl(page)).toBe(peerRelayUrl("peer-verified"));
  } finally {
    await close();
  }
});

test("@golden mesh-web-ui: the Web UI control and the mesh tab rail never overlap, desktop or mobile", async ({ webBrowser }) => {
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
      if (!webUiBox || !tabsBox) return;
      // The control moved out of the toolbar into the map layer, so the rail can
      // legitimately share its x-range; what must never happen is the two
      const intersects =
        webUiBox.x < tabsBox.x + tabsBox.width &&
        webUiBox.x + webUiBox.width > tabsBox.x &&
        webUiBox.y < tabsBox.y + tabsBox.height &&
        webUiBox.y + webUiBox.height > tabsBox.y;
      expect(intersects, `mesh-web-ui-action and mesh-tab-map must not overlap at ${viewport.width}px`).toBe(false);
      for (const [name, box] of [["mesh-web-ui-action", webUiBox], ["mesh-tab-map", tabsBox]] as const) {
        expect(box.x, `${name} left at ${viewport.width}px`).toBeGreaterThanOrEqual(-1);
        expect(box.x + box.width, `${name} right at ${viewport.width}px`).toBeLessThanOrEqual(viewport.width + 1);
      }
    } finally {
      await close();
    }
  }
});
