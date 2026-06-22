/**
 * 12-web / web-access — the Web UI modal picks the right primary URL per the
 * Tailscale-Serve auto-detect priority chain, and surfaces the curriculum hint
 * only when it falls back to an IP+port form.
 *
 * /api/network is mocked per scenario (route fulfill) so we can exercise every
 * shape — serve HTTPS on 443, serve HTTPS on a non-443 port, tailscale IP with
 * no serve, LAN-only — against a single live dev console. The asserted link IS
 * the string the QR is generated from (QRCode.toDataURL(link)), so checking the
 * link field verifies what the QR encodes.
 */
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

const WEB_URL = `${API_BASE}/web/`;
let token: string;

test.beforeAll(async () => {
  token = readDevToken();
});

type NetShape = Record<string, unknown>;

async function openWebUiWithNetwork(browser: Browser, net: NetShape): Promise<{ page: Page; close: () => Promise<void> }> {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  // Every get_network_info (bridge → GET /api/network) returns this scenario.
  await page.route("**/api/network", (route) =>
    route.fulfill({ contentType: "application/json", body: JSON.stringify(net) }),
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
  await page.click('[data-testid="web-ui-trigger"]');
  await page.waitForSelector('[data-testid="web-ui-modal"]', { timeout: 10_000 });
  return { page, close: () => ctx.close() };
}

const PORT = 7686;
const scenarios = [
  {
    name: "serve HTTPS on 443 → FQDN root is primary, no hint",
    net: {
      tailscale_https_url: "https://blackstar.test.ts.net",
      tailscale_ip: "100.64.0.9",
      local_ip: "192.168.1.20",
      api_port: PORT,
    },
    expectLink: "https://blackstar.test.ts.net/web/",
    fallback: false,
  },
  {
    name: "serve HTTPS on 7600 → port suffix preserved, no hint",
    net: { tailscale_https_url: "https://blackstar.test.ts.net:7600", api_port: PORT },
    expectLink: "https://blackstar.test.ts.net:7600/web/",
    fallback: false,
  },
  {
    name: "no serve, tailscale IP → IP+port primary, hint shown",
    net: { tailscale_ip: "100.64.0.9", local_ip: "192.168.1.20", api_port: PORT },
    expectLink: `http://100.64.0.9:${PORT}/web/`,
    fallback: true,
  },
  {
    name: "LAN only → LAN IP primary, hint shown",
    net: { local_ip: "192.168.1.20", api_port: PORT },
    expectLink: `http://192.168.1.20:${PORT}/web/`,
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

      const hint = page.locator('[data-testid="web-access-serve-hint"]');
      if (sc.fallback) {
        await expect(hint).toBeVisible();
        await expect(hint).toContainText(`tailscale serve --bg --https=443 http://127.0.0.1:${PORT}`);
      } else {
        await expect(hint).toHaveCount(0);
      }
    } finally {
      await close();
    }
  });
}
