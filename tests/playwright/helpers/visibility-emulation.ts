/**
 * Drive page-visibility / bfcache-style lifecycle events for gates that need
 * a suspended-then-foregrounded tab (S2b resync, S8 PWA lifecycle) without a
 * real OS-level tab switch, which Playwright/CDP cannot trigger directly.
 *
 * Browser-support caveats:
 * - `setHidden` overrides `document.visibilityState`/`document.hidden` via
 *   `Object.defineProperty` and fires a real `visibilitychange` event. This
 *   is a synthetic simulation, not a true OS occlusion signal — it does NOT
 *   by itself pause rAF/timers the way a real backgrounded tab would.
 * - `dispatchPageShow` fires a `PageTransitionEvent`; real bfcache restores
 *   are Chromium/WebKit-specific and cannot be triggered from JS, so this
 *   only exercises the app's `pageshow` LISTENER, not actual bfcache.
 * - `setLifecycleState` uses the CDP `Page.setWebLifecycleState` command
 *   (Chromium-only — no Firefox/WebKit equivalent). `webBrowser` in
 *   `fixtures.ts` launches Chromium (or msedge on Windows, also
 *   Chromium-based), so this is safe for this suite but must not be reused
 *   against a WebKit/Firefox target.
 */
import type { Page } from "@playwright/test";

export async function setHidden(page: Page, hidden: boolean): Promise<void> {
  await page.evaluate((isHidden) => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => (isHidden ? "hidden" : "visible"),
    });
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => isHidden,
    });
    document.dispatchEvent(new Event("visibilitychange"));
  }, hidden);
}

export async function dispatchPageShow(page: Page, persisted = false): Promise<void> {
  await page.evaluate((wasPersisted) => {
    window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: wasPersisted }));
  }, persisted);
}

/** Chromium-only: actually freezes the renderer's task/timer lifecycle. */
export async function setLifecycleState(page: Page, state: "active" | "frozen"): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send("Page.setWebLifecycleState", { state });
  } finally {
    await cdp.detach().catch(() => {});
  }
}
