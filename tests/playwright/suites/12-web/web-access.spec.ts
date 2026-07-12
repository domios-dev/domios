/**
 * WHAT: web access covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

const WEB_URL = `${API_BASE}/`;
let token: string;

test.beforeAll(async () => {
  token = readDevToken();
});

type NetShape = Record<string, unknown>;

async function openWebUiWithNetwork(browser: Browser, net: NetShape): Promise<{ page: Page; close: () => Promise<void> }> {
  const ctx = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    serviceWorkers: "block",
  });
  const page = await ctx.newPage();
  // Every get_network_info (bridge → GET /api/network) returns this scenario.
  await ctx.route("**/api/network", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify(net) }),
  );
  await ctx.route("**/api/mesh/peers", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({ paired: [], discovered: [], incoming_requests: [] }),
    }),
  );

  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 15_000 });
  await page.fill('[data-testid="web-token-input"]', token);
  await page.click('[data-testid="web-token-submit"]');
  await page
    .locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]')
    .first()
    .waitFor({ timeout: 20_000 });

  await page.click('[data-testid="nav-mesh"]');
  await page.click('[data-testid="mesh-web-ui-surface"]');
  await page.waitForSelector('[data-testid="web-ui-modal"]', { timeout: 10_000 });
  return { page, close: () => ctx.close() };
}

const PORT = 7686;
const scenarios = [
  {
    name: "serve HTTPS on 443 → FQDN root is primary, no hint",
    net: {
      tailscale_https_url: "https://blackstar.test.ts.net",
      tailscale_ip: "127.0.0.1",
      local_ip: "192.168.1.20",
      api_port: PORT,
    },
    expectLink: "https://blackstar.test.ts.net/",
    fallback: false,
  },
  {
    name: "serve HTTPS on 7600 → port suffix preserved, no hint",
    net: { tailscale_https_url: "https://blackstar.test.ts.net:7600", api_port: PORT },
    expectLink: "https://blackstar.test.ts.net:7600/",
    fallback: false,
  },
  {
    name: "no serve, stub peer address → IP+port primary, hint shown",
    net: { tailscale_ip: "127.0.0.1", local_ip: "192.168.1.20", api_port: PORT },
    expectLink: `http://127.0.0.1:${PORT}/`,
    fallback: true,
  },
  {
    name: "LAN only → LAN IP primary, hint shown",
    net: { local_ip: "192.168.1.20", api_port: PORT },
    expectLink: `http://192.168.1.20:${PORT}/`,
    fallback: true,
  },
];

for (const sc of scenarios) {
  test(`@golden web-access: ${sc.name}`, async ({ webBrowser }) => {
    const { page, close } = await openWebUiWithNetwork(webBrowser, sc.net);
    try {
      const expected = `${sc.expectLink}#token=${encodeURIComponent(token)}`;
      await expect(page.locator('[data-testid="web-ui-link"]')).toHaveValue(expected);

      // QR is derived from the same link string — assert it actually rendered.
      await expect(page.locator('img[alt="Web UI QR"]')).toHaveAttribute(
        "src",
        /^data:image\//,
      );

      if (sc.fallback) {
        const setup = page.locator('[data-testid="web-ui-setup"]');
        await expect(setup).toBeVisible();
        await expect(setup).toContainText("raw http:// address");
      } else {
        await expect(page.locator('[data-testid="web-ui-setup"]')).toHaveCount(0);
      }
    } finally {
      await close();
    }
  });
}
