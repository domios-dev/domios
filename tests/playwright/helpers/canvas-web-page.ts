/**
 * Canvas Playwright gates open the web UI without the shared `ui` fixture.
 * Fresh dev can land on the plan-empty canvas (no xterm/composer); connectUi's
 * waitForShell would time out before test setup runs.
 */
import type { Browser, BrowserContext, Page } from "@playwright/test";
import { readDevToken } from "./api-client";
import { API_BASE, isHealthy } from "./dev-console";

const PERF_FIXTURE_KEY = "canvas.perfFixtureN";
const PERF_FIXTURE_PROFILE_KEY = "canvas.perfFixtureProfile";
const SUBSTRATE_V2_KEY = "canvas.substrateV2";

export type CanvasWebSession = {
  page: Page;
  context: BrowserContext;
};

export type CanvasWebInit = {
  perfFixtureN?: number;
  perfFixtureProfile?: string;
  clearHarness?: boolean;
  viewport?: { width: number; height: number };
  // them (the SW proxies /api/* via its own fetch, bypassing page.route).
  serviceWorkers?: "allow" | "block";
};

export async function openCanvasWebPage(browser: Browser, init: CanvasWebInit = {}): Promise<CanvasWebSession> {
  if (!(await isHealthy())) {
    throw new Error("dev console health check failed before canvas web page");
  }
  const token = readDevToken();
  const viewport = init.viewport ?? { width: 1280, height: 800 };
  const context = await browser.newContext({ viewport, serviceWorkers: init.serviceWorkers });
  const page = await context.newPage();
  await page.addInitScript(
    ({ devToken, perfN, perfProfile, clearHarness, perfKey, profileKey, subKey }) => {
      try {
        localStorage.setItem(`codeterm-web-token:${window.location.origin}`, devToken);
        localStorage.removeItem("codeterm-chat-ui-default-enabled");
        if (clearHarness) {
          localStorage.removeItem(perfKey);
          localStorage.removeItem(profileKey);
          localStorage.removeItem(subKey);
        }
        if (perfN != null && perfN > 0) {
          localStorage.setItem(perfKey, String(perfN));
          localStorage.setItem(subKey, "true");
        }
        if (perfProfile) localStorage.setItem(profileKey, perfProfile);
      } catch {
        /* surfaced by canvas wait helpers */
      }
    },
    {
      devToken: token,
      perfN: init.perfFixtureN,
      perfProfile: init.perfFixtureProfile,
      clearHarness: init.clearHarness ?? false,
      perfKey: PERF_FIXTURE_KEY,
      profileKey: PERF_FIXTURE_PROFILE_KEY,
      subKey: SUBSTRATE_V2_KEY,
    },
  );
  await page.goto(`${API_BASE}/#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await page
    .locator(
      '[data-testid="plan-view-shell"], [data-testid="mission-control-live"], [data-testid="canvas-view"], [data-testid="canvas-empty"], [data-testid="workspace-item"]',
    )
    .first()
    .waitFor({ state: "visible", timeout: 90_000 });
  return { page, context };
}

export async function closeCanvasWebSession(session: CanvasWebSession): Promise<void> {
  await session.context.close();
}
