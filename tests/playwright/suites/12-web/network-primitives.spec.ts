/**
 * WHAT: network primitives covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 12-web.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";
import { NETWORK_DISRUPTION_ABORT_REGISTRY, networkDisruption } from "../../helpers/network-disruption";
import { createManagedWebSession } from "../../helpers/web-session";
import { delayRoute } from "../../helpers/route-delay";
import { dispatchPageShow, setHidden, setLifecycleState } from "../../helpers/visibility-emulation";

const WEB_URL = `${API_BASE}/`;

/** Starts a long-lived SSE fetch from the page and tracks its lifecycle on
 * `window.__sseState` — polled from Node so we never block on an open stream. */
async function openTrackedSse(page: Page, token: string): Promise<void> {
  await page.evaluate(({ key, t }) => {
    (window as unknown as { __sseState: unknown }).__sseState = { opened: false, closed: false };
    const state = (window as unknown as { __sseState: { opened: boolean; closed: boolean } }).__sseState;
    const controller = new AbortController();
    const win = window as unknown as Record<string, {
      sse: AbortController[];
      "pty-read": AbortController[];
      "pty-stream": AbortController[];
    }>;
    const registry = (win[key] ??= { sse: [], "pty-read": [], "pty-stream": [] });
    registry.sse.push(controller);
    void (async () => {
      try {
        const res = await fetch("/api/events/stream", {
          headers: { Authorization: `Bearer ${t}` },
          signal: controller.signal,
        });
        state.opened = res.ok;
        const reader = res.body!.getReader();
        for (;;) {
          const { done } = await reader.read();
          if (done) break;
        }
      } catch {
        /* falls through to closed=true below */
      } finally {
        const controllers = win[key]?.sse ?? [];
        const index = controllers.indexOf(controller);
        if (index >= 0) controllers.splice(index, 1);
        state.closed = true;
      }
    })();
  }, { key: NETWORK_DISRUPTION_ABORT_REGISTRY, t: token });
}

async function sseState(page: Page): Promise<{ opened: boolean; closed: boolean }> {
  return page.evaluate(
    () => (window as unknown as { __sseState: { opened: boolean; closed: boolean } }).__sseState,
  );
}

/** true if the fetch resolved (even with an HTTP error status); false if the
 * browser rejected it (route.abort() / offline) — the signal a severed route produces. */
async function fetchSucceeds(page: Page, url: string, token: string): Promise<boolean> {
  return page.evaluate(
    async ({ u, t }) => {
      try {
        await fetch(u, { headers: { Authorization: `Bearer ${t}` } });
        return true;
      } catch {
        return false;
      }
    },
    { u: url, t: token },
  );
}

test.describe("@golden network-disruption primitive", () => {
  let token: string;
  test.beforeAll(() => {
    token = readDevToken();
  });

  test("@golden selectively severs pty-read without touching sse, then cleanly restores", async ({ webBrowser }) => {
    const ctx = await webBrowser.newContext();
    const page = await ctx.newPage();
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    const disruption = networkDisruption(page);

    try {
      await disruption.sever("pty-read");
      expect(await fetchSucceeds(page, "/api/pty/pw-fake-id/read?offset=0", token)).toBe(false);
      expect(await fetchSucceeds(page, "/api/events/stream", token)).toBe(true);

      await disruption.restore("pty-read");
      expect(await fetchSucceeds(page, "/api/pty/pw-fake-id/read?offset=0", token)).toBe(true);
    } finally {
      await disruption.restoreAll();
      await ctx.close();
    }
  });

  test("@golden selective sever leaves an already-open SSE stream alone; hard sever kills it mid-flight", async ({
    webBrowser,
  }) => {
    const ctx = await webBrowser.newContext();
    const page = await ctx.newPage();
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    const disruption = networkDisruption(page);

    try {
      await openTrackedSse(page, token);
      await page.waitForFunction(
        () => (window as unknown as { __sseState?: { opened: boolean } }).__sseState?.opened === true,
        undefined,
        { timeout: 10_000 },
      );

      // Soft sever of a DIFFERENT target must not touch the already-open SSE connection:
      // waitForFunction is expected to time out (nothing closed it) within the window.
      await disruption.sever("pty-read");
      await page
        .waitForFunction(
          () => (window as unknown as { __sseState?: { closed: boolean } }).__sseState?.closed === true,
          undefined,
          { timeout: 1_500 },
        )
        .then(() => {
          throw new Error("SSE connection closed after severing an unrelated target");
        })
        .catch((e: Error) => {
          if (e.message.includes("SSE connection closed")) throw e;
        });
      expect((await sseState(page)).closed).toBe(false);
      await disruption.restore("pty-read");

      // not wait for its own reconnect cycle.
      await disruption.sever("sse", { hard: true });
      await page.waitForFunction(
        () => (window as unknown as { __sseState?: { closed: boolean } }).__sseState?.closed === true,
        undefined,
        { timeout: 10_000 },
      );
      expect((await sseState(page)).closed).toBe(true);
    } finally {
      await disruption.restoreAll();
      await ctx.close();
    }
  });

  test("@golden full offline severs and restores all /api/* traffic", async ({ webBrowser }) => {
    const ctx = await webBrowser.newContext();
    const page = await ctx.newPage();
    await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
    const disruption = networkDisruption(page);

    try {
      await disruption.sever("all");
      expect(await fetchSucceeds(page, "/api/health", token)).toBe(false);

      await disruption.restore("all");
      expect(await fetchSucceeds(page, "/api/health", token)).toBe(true);
    } finally {
      await disruption.restoreAll();
      await ctx.close();
    }
  });
});

test("@golden managed web-session distinguishes tab-reopen from a fresh browser restart", async ({ webBrowser }) => {
  const session = await createManagedWebSession(webBrowser);
  try {
    await expect(session.page.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first()).toBeVisible({
      timeout: 20_000,
    });
    const originalContext = session.context;

    const reopened = await session.reopenTab();
    expect(session.context).toBe(originalContext); // same context: real tab close/reopen
    await expect(reopened.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first()).toBeVisible({
      timeout: 20_000,
    });

    const restarted = await session.restartBrowser();
    expect(session.context).not.toBe(originalContext); // fresh context: full browser restart
    await expect(restarted.locator('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]').first()).toBeVisible({
      timeout: 20_000,
    });
  } finally {
    await session.close();
  }
});

test("@golden route-delay injection delays a named route by N ms, then cleanly removes", async ({ webBrowser }) => {
  const ctx = await webBrowser.newContext();
  const page = await ctx.newPage();
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
  const token = readDevToken();

  const delay = await delayRoute(page, "**/api/health", 1_500);
  try {
    const start = Date.now();
    expect(await fetchSucceeds(page, "/api/health", token)).toBe(true);
    expect(Date.now() - start).toBeGreaterThanOrEqual(1_400);
  } finally {
    await delay.remove();
  }

  const start = Date.now();
  expect(await fetchSucceeds(page, "/api/health", token)).toBe(true);
  expect(Date.now() - start).toBeLessThan(1_000);
  await ctx.close();
});

test("@golden visibility emulation drives visibilitychange, pageshow, and CDP lifecycle state", async ({
  webBrowser,
}) => {
  const ctx = await webBrowser.newContext();
  const page = await ctx.newPage();
  await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });

  await page.evaluate(() => {
    (window as unknown as { __events: string[] }).__events = [];
    const log = (name: string) => (window as unknown as { __events: string[] }).__events.push(name);
    document.addEventListener("visibilitychange", () => log(`visibilitychange:${document.hidden}`));
    window.addEventListener("pageshow", (e) => log(`pageshow:${(e as PageTransitionEvent).persisted}`));
  });

  await setHidden(page, true);
  await setHidden(page, false);
  await dispatchPageShow(page, true);

  const events = await page.evaluate(() => (window as unknown as { __events: string[] }).__events);
  expect(events).toEqual(["visibilitychange:true", "visibilitychange:false", "pageshow:true"]);

  // Chromium-only CDP lifecycle command — smoke that it round-trips without throwing.
  await setLifecycleState(page, "frozen");
  await setLifecycleState(page, "active");

  await ctx.close();
});
