/**
 * Dev-console lifecycle for Playwright runs.
 *
 * The dev console is the real Tauri app started via `./build.sh dev --wait`
 * (port 7686, data dir ~/.codeterm-dev/). On Windows its window is WebView2,
 * which exposes the Chrome DevTools Protocol when launched with
 * `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<CDP_PORT>`
 * — Playwright connects to the REAL app over CDP, no separate browser.
 */
import { execFileSync } from "node:child_process";
import { chromium, type Browser, type Page } from "@playwright/test";
import path from "node:path";

export const API_BASE = "http://127.0.0.1:7686";
export const CDP_PORT = 9223;
export const CDP_BASE = `http://127.0.0.1:${CDP_PORT}`;

export const REPO_ROOT = path.resolve(__dirname, "..", "..", "..");

export async function isHealthy(): Promise<boolean> {
  try {
    const r = await fetch(`${API_BASE}/api/health`, { signal: AbortSignal.timeout(3000) });
    if (!r.ok) return false;
    const body = (await r.json()) as { status?: string };
    return body.status === "ok";
  } catch {
    return false;
  }
}

export async function isCdpUp(): Promise<boolean> {
  try {
    const r = await fetch(`${CDP_BASE}/json/version`, { signal: AbortSignal.timeout(3000) });
    return r.ok;
  } catch {
    return false;
  }
}

function bash(args: string[], timeoutMs: number): void {
  // build.sh is the only sanctioned entry point (never raw cargo/pnpm tauri).
  // Requires git-bash on PATH — the standard CodeTerm dev setup on Windows.
  execFileSync("bash", args, {
    cwd: REPO_ROOT,
    stdio: "inherit",
    timeout: timeoutMs,
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    },
  });
}

export function killDevConsole(): void {
  bash(["build.sh", "kill"], 60_000);
}

/**
 * Make sure a dev console with a reachable CDP endpoint is running.
 * Reuses a running one when both health and CDP answer; otherwise restarts
 * it (a console started without the CDP env var can't be adopted — the
 * debugging port is fixed at WebView2 creation).
 */
export async function ensureDevConsole(): Promise<void> {
  if ((await isHealthy()) && (await isCdpUp())) return;
  killDevConsole();
  bash(["build.sh", "dev", "--wait"], 600_000);
  if (!(await isHealthy())) {
    throw new Error("dev console did not become healthy after ./build.sh dev --wait");
  }
  for (let i = 0; i < 20; i++) {
    if (await isCdpUp()) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(
    `dev console is healthy but CDP (${CDP_BASE}) never came up — ` +
      "was the app launched without WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS?",
  );
}

export interface UiSession {
  browser: Browser;
  page: Page;
}

/** Attach to the running dev console window over CDP and wait until the
 * shell has actually rendered — on a cold start the WebView is up while
 * Vite is still compiling, so the page can be blank for many seconds. */
export async function connectUi(): Promise<UiSession> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= 3; attempt++) {
    let browser: Browser | undefined;
    try {
      await ensureDevConsole();
      browser = await chromium.connectOverCDP(CDP_BASE);
      const pages = browser.contexts().flatMap((c) => c.pages()).filter((p) => !p.isClosed());
      const page =
        pages.find((p) => /localhost|tauri/.test(p.url())) ??
        pages[0] ??
        (() => {
          throw new Error("no page exposed over CDP — dev console window missing?");
        })();
      await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
      // First terminal mounts after session restore + PTY spawn.
      await page.waitForSelector(".xterm", { state: "visible", timeout: 90_000 });
      return { browser, page };
    } catch (e) {
      lastError = e;
      await browser?.close().catch(() => {});
      const msg = String(e);
      if (!/Target page|context or browser has been closed|no page exposed/.test(msg) || attempt === 3) {
        throw e;
      }
      killDevConsole();
    }
  }
  throw lastError;
}
