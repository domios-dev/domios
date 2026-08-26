/**
 * Selecting the app document on a WebView2 CDP endpoint by identity.
 *
 * A WebView2 environment can expose several page targets (the app window, an
 * about:blank popup, a devtools target). Position names none of them, so a
 * stray target makes a healthy app look dead.
 */
import { chromium, type Page } from "@playwright/test";
import type { DevInstance } from "./dev-instance";

export const APP_ROOT_SELECTOR = "#root > *";
export const APP_READY_TIMEOUT_MS = 3_000;
export const CDP_CONNECT_TIMEOUT_MS = 3_000;

export type AppPageCriterion = "app-origin" | "app-root";

export type AppPageCandidate = {
  url: string;
  index: number;
  criterion: AppPageCriterion;
};

export type CdpAppReadiness =
  | { ready: true; matched: string; criterion: AppPageCriterion }
  | { ready: false; reason: string };

const NON_APP_SCHEMES = ["about:", "devtools:", "chrome:", "chrome-extension:", "edge:", "data:"];

function appOrigins(instance: DevInstance): string[] {
  return [
    `http://127.0.0.1:${instance.vitePort}`,
    `http://localhost:${instance.vitePort}`,
    `http://127.0.0.1:${instance.apiPort}`,
    `http://localhost:${instance.apiPort}`,
    "tauri://localhost",
    "https://tauri.localhost",
  ];
}

// URL.origin is "null" for non-special schemes such as tauri:, so compare host.
function schemeAndHost(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    return `${parsed.protocol}//${parsed.host}`.toLowerCase();
  } catch {
    return undefined;
  }
}

/** The frontend this named instance serves — never a sibling instance's port. */
export function isAppOriginUrl(url: string, instance: DevInstance): boolean {
  const origin = schemeAndHost(url);
  return origin !== undefined && appOrigins(instance).includes(origin);
}

export function isProbeCandidateUrl(url: string): boolean {
  const normalized = url.trim().toLowerCase();
  if (!normalized) return false;
  return !NON_APP_SCHEMES.some((scheme) => normalized.startsWith(scheme));
}

/** Instance-origin pages first, then any document that could carry the root. */
export function rankAppPageCandidates(urls: string[], instance: DevInstance): AppPageCandidate[] {
  const eligible = urls
    .map((url, index) => ({ url, index }))
    .filter((candidate) => isProbeCandidateUrl(candidate.url));
  return [
    ...eligible
      .filter((candidate) => isAppOriginUrl(candidate.url, instance))
      .map((candidate) => ({ ...candidate, criterion: "app-origin" as const })),
    ...eligible
      .filter((candidate) => !isAppOriginUrl(candidate.url, instance))
      .map((candidate) => ({ ...candidate, criterion: "app-root" as const })),
  ];
}

export function noAppPageReason(urls: string[]): string {
  const listed = urls.length === 0 ? "none" : urls.join(", ");
  return `no CDP page carries ${APP_ROOT_SELECTOR}; open page targets: [${listed}]`;
}

async function carriesAppRoot(page: Page, timeoutMs: number): Promise<boolean> {
  try {
    await page.locator(APP_ROOT_SELECTOR).first().waitFor({ state: "attached", timeout: timeoutMs });
    return true;
  } catch {
    return false;
  }
}

/** Reports whether the app document is rendered, naming the matched page. */
export async function probeCdpAppReadiness(
  instance: DevInstance,
  timeoutMs = APP_READY_TIMEOUT_MS,
): Promise<CdpAppReadiness> {
  const browser = await chromium
    .connectOverCDP(instance.cdpBase, { timeout: CDP_CONNECT_TIMEOUT_MS })
    .catch((error: unknown) => ({ cdpError: error }));
  if ("cdpError" in browser) {
    return { ready: false, reason: `cannot attach to CDP at ${instance.cdpBase}: ${browser.cdpError}` };
  }
  try {
    const pages = browser
      .contexts()
      .flatMap((context) => context.pages())
      .filter((page) => !page.isClosed());
    const urls = pages.map((page) => page.url());
    for (const candidate of rankAppPageCandidates(urls, instance)) {
      if (await carriesAppRoot(pages[candidate.index], timeoutMs)) {
        return { ready: true, matched: candidate.url, criterion: candidate.criterion };
      }
    }
    return { ready: false, reason: noAppPageReason(urls) };
  } catch (error) {
    return { ready: false, reason: `CDP page inspection failed: ${error}` };
  } finally {
    await browser.close().catch(() => {});
  }
}
