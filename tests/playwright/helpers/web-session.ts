/**
 * Managed web session — the `webBrowser`-fixture equivalent of
 * `ManagedUiSession.reconnect()` (desktop CDP). Web specs need TWO distinct
 * reattach shapes, not one:
 *
 * - `reopenTab()`  — close the Page, open a new one in the SAME
 *   BrowserContext. localStorage (the web auth token, `codeterm-web-token:*`
 *   in `src/lib/webApi.ts`) survives, matching a real browser tab
 *   close-and-reopen; sessionStorage (mesh relay target) does not, also
 *   matching real tab behavior.
 * - `restartBrowser()` — close the whole BrowserContext, open a fresh one on
 *   the same Browser. localStorage is gone too, so the token gate
 *   (`src/components/WebGate/WebGate.tsx`) reappears — matching a full
 *   browser/profile restart. Bootstraps auth the same way `connectWebUi`
 *   does in `dev-console.ts`: token seeded via `addInitScript` AND passed in
 *   the URL fragment, consumed once by `WebGate`.
 */
import type { Browser, BrowserContext, Page } from "@playwright/test";
import { WEB_URL } from "./dev-console";
import { readDevToken } from "./api-client";

export interface ManagedWebSession {
  readonly page: Page;
  readonly context: BrowserContext;
  /** Tab close-and-reopen: same context, localStorage survives. */
  reopenTab(): Promise<Page>;
  /** Full browser restart: fresh context, localStorage gone, token gate reappears. */
  restartBrowser(): Promise<Page>;
  close(): Promise<void>;
}

export interface ManagedWebSessionOptions {
  viewport?: { width: number; height: number };
  token?: string;
}

async function waitForWebShell(page: Page): Promise<void> {
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await page.waitForSelector(
    '.xterm:visible, [data-testid="chat-composer"]:visible, [data-testid="peer-picker-button"]:visible, [data-testid="web-sidebar-open"]:visible',
    { state: "visible", timeout: 90_000 },
  );
}

async function seedToken(page: Page, token: string): Promise<void> {
  await page.addInitScript((t) => {
    try {
      localStorage.setItem(`codeterm-web-token:${window.location.origin}`, t);
    } catch {
      /* private browsing — falls back to the URL-fragment token below */
    }
  }, token);
}

async function bootstrapAuthedPage(context: BrowserContext, token: string): Promise<Page> {
  const page = await context.newPage();
  await seedToken(page, token);
  await page.goto(`${WEB_URL}#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await waitForWebShell(page);
  return page;
}

export async function createManagedWebSession(
  browser: Browser,
  opts: ManagedWebSessionOptions = {},
): Promise<ManagedWebSession> {
  const token = opts.token ?? readDevToken();
  const viewport = opts.viewport ?? { width: 1280, height: 800 };

  let context = await browser.newContext({ viewport });
  let page = await bootstrapAuthedPage(context, token);

  return {
    get page() {
      return page;
    },
    get context() {
      return context;
    },

    async reopenTab() {
      const sameContext = context;
      await page.close().catch(() => {});
      page = await sameContext.newPage();
      // Token already lives in this context's localStorage — no fragment needed.
      await page.goto(WEB_URL, { waitUntil: "domcontentloaded" });
      await waitForWebShell(page);
      return page;
    },

    async restartBrowser() {
      await context.close().catch(() => {});
      context = await browser.newContext({ viewport });
      page = await bootstrapAuthedPage(context, token);
      return page;
    },

    async close() {
      await context.close().catch(() => {});
    },
  };
}
