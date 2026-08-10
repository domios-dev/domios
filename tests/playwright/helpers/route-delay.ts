/**
 * Delay a named API route by N ms before letting it through — for gates that
 * assert the cached shell paints before the daemon actually responds (S4,
 * frontend persistence & caching).
 */
import type { Page } from "@playwright/test";

export interface RouteDelay {
  remove(): Promise<void>;
}

export async function delayRoute(page: Page, urlGlob: string, delayMs: number): Promise<RouteDelay> {
  const handler = async (route: import("@playwright/test").Route) => {
    // PW_JUSTIFIED_SLEEP: stimulus — the injected latency IS the test input.
    await new Promise((resolve) => setTimeout(resolve, delayMs));
    await route.continue();
  };
  await page.route(urlGlob, handler);
  return {
    remove: () => page.unroute(urlGlob, handler),
  };
}
