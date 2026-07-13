/**
 * Dev-console lifecycle for Playwright runs.
 *
 * The dev console is the real Tauri app started via `./build.sh dev --wait`
 * in a named Playwright instance. On Windows its window is WebView2,
 * which exposes the Chrome DevTools Protocol from the dev Tauri config.
 * Playwright connects to the REAL app over CDP, no separate browser.
 */
import { execFileSync } from "node:child_process";
import { chromium, type Browser, type Page } from "@playwright/test";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { readDevToken } from "./api-client";
import {
  currentDevInstance,
  devStartArgs,
  playwrightCargoTargetDir,
  type DevInstance,
} from "./dev-instance";

const DEFAULT_INSTANCE = currentDevInstance();
export const E2E_PORT = String(DEFAULT_INSTANCE.apiPort);
export const API_BASE = DEFAULT_INSTANCE.apiBase;
export const CDP_PORT = DEFAULT_INSTANCE.cdpPort;
export const CDP_BASE = DEFAULT_INSTANCE.cdpBase;
export const WEB_URL = DEFAULT_INSTANCE.webUrl;
const WEB_MODE = process.env.CT_E2E_WEB_MODE === "1";
const WEB_UI_READY_TIMEOUT_MS = 90_000;

export const REPO_ROOT = process.env.CT_E2E_DEV_ROOT
  ? path.resolve(process.env.CT_E2E_DEV_ROOT)
  : path.resolve(__dirname, "..", "..", "..");

export async function isHealthy(instance = currentDevInstance()): Promise<boolean> {
  try {
    const r = await fetch(`${instance.apiBase}/api/health`, { signal: AbortSignal.timeout(3000) });
    if (!r.ok) return false;
    const body = (await r.json()) as {
      status?: string;
      instance?: {
        name?: string;
        api_port?: string | number;
        vite_port?: string | number;
        cdp_port?: string | number;
        identifier?: string;
      };
    };
    return (
      body.status === "ok" &&
      body.instance?.name === instance.name &&
      String(body.instance?.api_port) === String(instance.apiPort) &&
      String(body.instance?.vite_port) === String(instance.vitePort) &&
      String(body.instance?.cdp_port) === String(instance.cdpPort) &&
      body.instance?.identifier === instance.identifier
    );
  } catch {
    return false;
  }
}

export function isWebUiDocument(contentType: string | null, body: string): boolean {
  return Boolean(contentType?.includes("text/html") && /<div\s+id=["']root["']/.test(body));
}

export async function isWebUiReady(instance = currentDevInstance()): Promise<boolean> {
  try {
    const response = await fetch(`${instance.webUrl}?e2e=1`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!response.ok) return false;
    return isWebUiDocument(response.headers.get("content-type"), await response.text());
  } catch {
    return false;
  }
}

async function waitForWebUi(instance: DevInstance): Promise<boolean> {
  const deadline = Date.now() + WEB_UI_READY_TIMEOUT_MS;
  do {
    if (await isWebUiReady(instance)) return true;
    await new Promise((resolve) => setTimeout(resolve, 500));
  } while (Date.now() < deadline);
  return false;
}

export async function isCdpUp(instance = currentDevInstance()): Promise<boolean> {
  try {
    const r = await fetch(`${instance.cdpBase}/json/version`, { signal: AbortSignal.timeout(3000) });
    return r.ok;
  } catch {
    return false;
  }
}

async function isCdpAppReady(instance: DevInstance, timeoutMs = 3000): Promise<boolean> {
  if (process.platform !== "win32") return true;
  try {
    const browser = await chromium.connectOverCDP(instance.cdpBase, { timeout: timeoutMs });
    try {
      const page = browser.contexts()[0]?.pages()[0];
      if (!page) return false;
      await page.waitForLoadState("load", { timeout: timeoutMs });
      await page.locator("#root > *").first().waitFor({ state: "attached", timeout: timeoutMs });
      return true;
    } finally {
      await browser.close();
    }
  } catch {
    return false;
  }
}

// from PowerShell — build.sh then dies in a WSL world. Pin git-bash.
const BASH_EXE =
  process.platform === "win32"
    ? ["C:\\Program Files\\Git\\bin\\bash.exe", "C:\\Program Files (x86)\\Git\\bin\\bash.exe"].find(
        (p) => fs.existsSync(p),
      ) ??
      (() => {
        throw new Error("git-bash not found — install Git for Windows");
      })()
    : "bash";

function bash(instance: DevInstance, args: string[], timeoutMs: number): void {
  const cargoTargetDir = playwrightCargoTargetDir(instance);
  // build.sh is the only sanctioned entry point (never raw cargo/pnpm tauri).
  execFileSync(BASH_EXE, args, {
    cwd: REPO_ROOT,
    stdio: "inherit",
    timeout: timeoutMs,
    env: {
      ...process.env,
      CODETERM_INSTANCE: instance.name,
      CODETERM_API_PORT: String(instance.apiPort),
      CODETERM_VITE_PORT: String(instance.vitePort),
      CODETERM_CDP_PORT: String(instance.cdpPort),
      CT_E2E_DATADIR: instance.dataDirName,
      CT_E2E_PORT: String(instance.apiPort),
      CARGO_TARGET_DIR: cargoTargetDir ?? process.env.CARGO_TARGET_DIR,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${instance.cdpPort}`,
      VITE_CODETERM_TEST: "1",
      CODETERM_TEST_CLOCK: "1",
    },
  });
}

export function killDevConsole(instance = currentDevInstance()): void {
  markCleanRestartBeforeKill(instance);
  // A failing kill must not abort setup: on a fresh/CI runner there is nothing
  // to kill, and Windows process enumeration can EPERM on the protected
  // %USERPROFILE%\AppData junction. killDevLeftovers + `dev --wait` recover.
  try {
    bash(instance, ["build.sh", "kill"], 60_000);
  } catch (e) {
    console.warn(`[dev-console] build.sh kill failed (continuing): ${e}`);
  }
  killDevLeftovers(instance);
}

function markCleanRestartBeforeKill(instance: DevInstance): void {
  try {
    const token = readDevToken(instance);
    execFileSync(
      process.execPath,
      [
        "-e",
        [
          "const [url, token] = process.argv.slice(1);",
          "fetch(url, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(3000) })",
          "  .catch(() => undefined);",
        ].join(""),
        `${instance.apiBase}/api/debug/shutdown/mark-clean-restart`,
        token,
      ],
      { timeout: 5_000, stdio: "ignore" },
    );
  } catch {
    /* best effort — real crashes must still surface */
  }
}

/**
 * Windows recovery for the recurring crash-leftover states build.sh kill
 * can't see: orphan dev binaries (no 7686 listener but still holding
 * target\debug\codeterm.exe → relink fails with PermissionDenied) and a
 * surviving Vite process. Scoped hard to the named instance — prod and the
 * interactive dev instance are untouchable by construction.
 */
function killDevLeftovers(instance: DevInstance): void {
  if (process.platform !== "win32") return;
  try {
    execFileSync(
      "powershell",
      windowsCleanupArgs(instance),
      {
        timeout: 30_000,
        stdio: "inherit",
      },
    );
  } catch {
    /* best effort — the dev --wait retry decides success */
  }
}

function windowsCleanupArgs(instance: DevInstance): string[] {
  const cargoTargetDir = playwrightCargoTargetDir(instance);
  return [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    path.join(REPO_ROOT, "scripts", "windows-dev-cleanup.ps1"),
    "-Root",
    REPO_ROOT,
    "-TargetDebug",
    path.join(REPO_ROOT, cargoTargetDir ?? "src-tauri/target", "debug"),
    "-Identifier",
    instance.identifier,
    "-Instance",
    instance.name,
    "-VitePort",
    String(instance.vitePort),
    "-ApiPort",
    String(instance.apiPort),
    "-CdpPort",
    String(instance.cdpPort),
  ];
}

function gatewayLogDirs(): string[] {
  if (process.platform !== "win32") return [];
  const localAppData = process.env.LOCALAPPDATA;
  const userProfile = process.env.USERPROFILE;
  const programData = process.env.ProgramData;
  return [
    localAppData && path.join(localAppData, "CodeTerm", "build-gateway", "jobs"),
    userProfile && path.join(userProfile, "AppData", "Local", "CodeTerm", "build-gateway", "jobs"),
    programData && path.join(programData, "CodeTerm", "build-gateway", "jobs"),
  ].filter((dir): dir is string => Boolean(dir));
}

function dumpBuildGatewayLogs(reason: string): void {
  if (process.platform !== "win32") return;
  console.warn(`[dev-console] ${reason}; recent build-gateway logs:`);
  for (const dir of gatewayLogDirs()) {
    try {
      const logs = fs
        .readdirSync(dir)
        .filter((name) => name.endsWith(".log"))
        .map((name) => path.join(dir, name))
        .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)
        .slice(0, 4);
      if (logs.length === 0) continue;
      for (const log of logs) {
        console.warn(`[dev-console] --- ${log}`);
        const lines = fs.readFileSync(log, "utf8").split(/\r?\n/).slice(-160);
        console.warn(lines.join("\n"));
      }
      return;
    } catch {
      /* try the next candidate directory */
    }
  }
  console.warn("[dev-console] no readable build-gateway log directory found");
}

/**
 * Make sure a dev console with a reachable CDP endpoint is running.
 * Reuses a running one when both health and CDP answer; otherwise restarts
 * it (a console started without the CDP env var can't be adopted — the
 * debugging port is fixed at WebView2 creation).
 */
export async function ensureDevConsole(instance = currentDevInstance()): Promise<void> {
  const usesWebUi = WEB_MODE || process.platform !== "win32";
  if (await isHealthy(instance)) {
    if (usesWebUi && (await waitForWebUi(instance))) return;
    if (!usesWebUi && (await isCdpUp(instance)) && (await isCdpAppReady(instance))) return;
  }
  killDevConsole(instance);
  try {
    bash(instance, devStartArgs(), 600_000);
  } catch (first) {
    dumpBuildGatewayLogs(`first ./build.sh dev --wait failed: ${first}`);
    if ((first as NodeJS.ErrnoException).code === "ETIMEDOUT") throw first;
    // locking the binary, vite squatting 1420) — sweep and retry once.
    killDevLeftovers(instance);
    try {
      bash(instance, devStartArgs(), 600_000);
    } catch (second) {
      dumpBuildGatewayLogs(`second ./build.sh dev --wait failed: ${second}`);
      throw second;
    }
  }
  if (!(await isHealthy(instance))) {
    throw new Error("dev console did not become healthy after ./build.sh dev --wait");
  }
  if (usesWebUi) {
    if (await waitForWebUi(instance)) return;
    throw new Error(
      `dev console is healthy but web UI (${instance.webUrl}) did not become ready`,
    );
  }
  for (let i = 0; i < 20; i++) {
    if ((await isCdpUp(instance)) && (await isCdpAppReady(instance))) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(
    `dev console is healthy but WebView2 (${instance.cdpBase}) did not finish loading the app`,
  );
}

export interface UiSession {
  browser: Browser;
  page: Page;
}

async function completeFirstRunPickerIfPresent(page: Page): Promise<void> {
  const picker = page.locator('[data-testid="first-run-general-picker"]');
  if (!(await picker.isVisible({ timeout: 1_000 }).catch(() => false))) return;

  const confirm = page.locator('[data-testid="agent-picker-confirm"]');
  await confirm.waitFor({ state: "visible", timeout: 30_000 });
  const canvasButton = page.locator('[data-testid="canvas-toolbar-button"][aria-pressed="true"]');
  if (await canvasButton.isVisible().catch(() => false)) {
    await canvasButton.click({ timeout: 30_000 });
  }
  await expectReceivesPointerEvents(confirm, 30_000);
  await confirm.click({ timeout: 30_000 });
  await picker.waitFor({ state: "detached", timeout: 30_000 });
}

async function expectReceivesPointerEvents(locator: ReturnType<Page["locator"]>, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      await locator.click({ trial: true, timeout: 1_000 });
      return;
    } catch (e) {
      lastError = e;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw lastError;
}

async function waitForShellOnce(page: Page): Promise<void> {
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await page
    .evaluate(() =>
      "serviceWorker" in navigator
        ? Promise.race([
            navigator.serviceWorker.ready.then(() => undefined),
            new Promise<void>((resolve) => setTimeout(resolve, 1_000)),
          ])
        : undefined,
    )
    .catch(() => {
      /* no SW in this context (desktop CDP) or registration still pending — non-fatal */
    });
  await page.evaluate(() => {
    try {
      localStorage.removeItem("codeterm-chat-ui-default-enabled");
    } catch {
      /* localStorage may be unavailable; tests that need it will surface it */
    }
  });
  await completeFirstRunPickerIfPresent(page);
  await page
    .waitForSelector('.xterm:visible, [data-testid="chat-composer"]:visible', {
      state: "visible",
      timeout: 90_000,
    });
}

async function waitForShell(page: Page): Promise<void> {
  try {
    await waitForShellOnce(page);
  } catch (e) {
    if (!/Execution context was destroyed|Target closed/.test(String(e))) throw e;
    await page.waitForLoadState("domcontentloaded");
    await waitForShellOnce(page);
  }
}

async function connectDesktopUi(instance: DevInstance): Promise<UiSession> {
  const browser = await chromium.connectOverCDP(instance.cdpBase);
  const pages = browser.contexts().flatMap((c) => c.pages()).filter((p) => !p.isClosed());
  const page =
    pages.find((p) => /localhost|tauri/.test(p.url())) ??
    pages[0] ??
    (() => {
      throw new Error("no page exposed over CDP — dev console window missing?");
    })();
  // xterm a11y text layers (the DOM-oracle surface) are runtime-gated by
  // ?a11y — force it so oracles never depend on vite env plumbing.
  try {
    const url = new URL(page.url());
    if (!url.searchParams.has("e2e")) {
      url.searchParams.set("e2e", "1");
      await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
    }
  } catch {
    /* non-URL page (about:blank) — waitForShell will surface real problems */
  }
  await waitForShell(page);
  return { browser, page };
}

async function connectWebUi(instance: DevInstance): Promise<UiSession> {
  const token = readDevToken(instance);
  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  await page.addInitScript((devToken) => {
    try {
      localStorage.setItem(`codeterm-web-token:${window.location.origin}`, devToken);
      localStorage.removeItem("codeterm-chat-ui-default-enabled");
    } catch {
      /* localStorage may be unavailable; tests that need it will surface it */
    }
  }, token);
  await page.goto(`${instance.webUrl}?e2e=1#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await waitForShell(page);
  return { browser, page };
}

/** Attach to the running dev console window over CDP and wait until the
 * shell has actually rendered — on a cold start the WebView is up while
 * Vite is still compiling, so the page can be blank for many seconds. */
export async function connectUi(instance = currentDevInstance()): Promise<UiSession> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= 3; attempt++) {
    let browser: Browser | undefined;
    try {
      await ensureDevConsole(instance);
      const session =
        process.platform === "win32" ? await connectDesktopUi(instance) : await connectWebUi(instance);
      browser = session.browser;
      return session;
    } catch (e) {
      lastError = e;
      await browser?.close().catch(() => {});
      const msg = String(e);
      if (!/Target page|context or browser has been closed|no page exposed/.test(msg) || attempt === 3) {
        throw e;
      }
      killDevConsole(instance);
    }
  }
  throw lastError;
}

function portIsFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = net.createConnection({ host: "127.0.0.1", port });
    socket.setTimeout(500);
    socket.once("connect", () => {
      socket.destroy();
      resolve(false);
    });
    const free = () => {
      socket.destroy();
      resolve(true);
    };
    socket.once("error", free);
    socket.once("timeout", free);
  });
}

function residualOwnedProcessIds(instance: DevInstance): number[] {
  if (process.platform !== "win32") return [];
  const output = execFileSync("powershell", [...windowsCleanupArgs(instance), "-PlanOnly"], {
    encoding: "utf8",
    timeout: 30_000,
  });
  const plan = JSON.parse(output) as { selected_processes?: number[] };
  return plan.selected_processes ?? [];
}

export async function assertDevConsoleStopped(instance: DevInstance): Promise<void> {
  const ports = [instance.apiPort, instance.vitePort, instance.cdpPort];
  const deadline = Date.now() + 15_000;
  while (Date.now() <= deadline) {
    if (
      (await Promise.all(ports.map(portIsFree))).every(Boolean) &&
      residualOwnedProcessIds(instance).length === 0
    ) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  const occupied = [];
  for (const port of ports) {
    if (!(await portIsFree(port))) occupied.push(port);
  }
  const processes = residualOwnedProcessIds(instance);
  throw new Error(
    `Playwright instance ${instance.name || "default"} leaked listeners [${occupied.join(", ")}] or processes [${processes.join(", ")}]`,
  );
}
