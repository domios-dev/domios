/**
 * Dev-console lifecycle for Playwright runs.
 *
 * The dev console is the real Tauri app started via `./build.sh dev --wait`
 * in a named Playwright instance. On Windows its window is WebView2,
 * which exposes the Chrome DevTools Protocol from the dev Tauri config.
 * Playwright connects to the REAL app over CDP, no separate browser.
 */
import { execFileSync, spawn } from "node:child_process";
import { chromium, type Browser, type Page } from "@playwright/test";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { readDevToken } from "./api-client";
import { probeCdpAppReadiness, type CdpAppReadiness } from "./cdp-app-page";
import {
  buildDevInstanceEnv,
  currentDevInstance,
  devInstanceTeardownPorts,
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
const STOP_VERIFY_TIMEOUT_MS = process.platform === "win32" ? 30_000 : 15_000;
const SERVICE_WORKER_READY_TIMEOUT_MS = 30_000;

/** Adopt-only entry points never start an instance; they name the one that does. */
export const STAND_UP_INSTANCE_HINT =
  "stand the named instance up with `pnpm test:pw:instance` (adopt-only runners never start one)";

export const REPO_ROOT = process.env.CT_E2E_DEV_ROOT
  ? path.resolve(process.env.CT_E2E_DEV_ROOT)
  : path.resolve(__dirname, "..", "..", "..");

export function emitDevConsoleProgress(phase: string, detail: string): void {
  console.log(`[dev-console] ${phase}: ${detail}`);
}

let cachedHeadSha: string | undefined;

/** The commit under test. Memoised: a run is about exactly one HEAD. */
export function testedHeadSha(): string {
  if (cachedHeadSha) return cachedHeadSha;
  let output: string;
  try {
    output = execFileSync("git", ["rev-parse", "HEAD"], { cwd: REPO_ROOT, encoding: "utf8" }).trim();
  } catch (error) {
    throw new Error(`cannot resolve the tested HEAD in ${REPO_ROOT}: ${error}`);
  }
  if (!/^[0-9a-f]{40}$/.test(output)) {
    throw new Error(`cannot resolve the tested HEAD in ${REPO_ROOT}: git returned "${output}"`);
  }
  cachedHeadSha = output;
  return output;
}

export interface HealthBody {
  status?: string;
  build_sha?: string;
  instance?: {
    name?: string;
    api_port?: string | number;
    vite_port?: string | number;
    cdp_port?: string | number;
    identifier?: string;
  };
}

export type HealthVerdict =
  | { ok: true }
  | { ok: false; reason: "status" | "identity" }
  | { ok: false; reason: "build"; actual: string; expected: string };

/**
 * Identity AND build. Without the build comparison the adopt path silently
 * reuses an instance compiled at a different HEAD.
 */
export function evaluateHealth(body: HealthBody, instance: DevInstance, expectedSha: string): HealthVerdict {
  if (body.status !== "ok") return { ok: false, reason: "status" };
  const identityMatches =
    body.instance?.name === instance.name &&
    String(body.instance?.api_port) === String(instance.apiPort) &&
    String(body.instance?.vite_port) === String(instance.vitePort) &&
    String(body.instance?.cdp_port) === String(instance.cdpPort) &&
    body.instance?.identifier === instance.identifier;
  if (!identityMatches) return { ok: false, reason: "identity" };
  if (body.build_sha !== expectedSha) {
    return { ok: false, reason: "build", actual: body.build_sha ?? "absent", expected: expectedSha };
  }
  return { ok: true };
}

export function healthMismatchMessage(instance: DevInstance, verdict: HealthVerdict): string {
  if (verdict.ok) return "";
  if (verdict.reason === "build") {
    return `Playwright instance ${instance.name || "default"} at ${instance.apiBase} was built at ${verdict.actual}, not the tested HEAD ${verdict.expected}; rebuild it through the Playwright harness before adopting it`;
  }
  return `Playwright instance ${instance.name || "default"} is not healthy at ${instance.apiBase} (${verdict.reason} mismatch); ${STAND_UP_INSTANCE_HINT}`;
}

function progress(phase: string, detail: string): void {
  emitDevConsoleProgress(phase, detail);
}

export function nonAdoptableCdpError(instance: DevInstance): Error {
  return new Error(
    `healthy Playwright dev instance lacks the CDP endpoint at ${instance.cdpBase}; do not run Playwright against this manually started dev. Stop it and rerun via the Playwright harness, or expose WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=${instance.cdpPort}.`,
  );
}

export async function assertReusableDevConsole(instance = currentDevInstance()): Promise<void> {
  const usesWebUi = WEB_MODE || process.platform !== "win32";
  progress("health probe", `checking ${instance.apiBase}/api/health at HEAD ${testedHeadSha()}`);
  const verdict = await probeHealth(instance);
  if (!verdict.ok) throw new Error(healthMismatchMessage(instance, verdict));
  assertFrontendBuildIdentity();

  if (usesWebUi) {
    progress("web-ui probe", `checking ${instance.webUrl}`);
    if (await isWebUiReady(instance)) return;
    throw new Error(`healthy Playwright instance has no ready web UI at ${instance.webUrl}`);
  }

  progress("CDP probe", `checking ${instance.cdpBase}/json/version`);
  if (!(await isCdpUp(instance))) throw nonAdoptableCdpError(instance);
  progress("CDP app readiness", "checking the WebView2 document");
  const readiness = await cdpAppReadiness(instance);
  if (!readiness.ready) {
    throw new Error(
      `healthy Playwright instance exposes CDP at ${instance.cdpBase}, but its app is not ready (${readiness.reason}); ${STAND_UP_INSTANCE_HINT}`,
    );
  }
}

export function assertFrontendBuildIdentity(): void {
  const identityFile = path.join(REPO_ROOT, "dist", "build-identity.json");
  let data: { buildHash?: unknown };
  try {
    data = JSON.parse(fs.readFileSync(identityFile, "utf8")) as { buildHash?: unknown };
  } catch (error) {
    throw new Error(`Playwright preparation cannot read frontend build identity at ${identityFile}: ${error}`);
  }
  if (typeof data.buildHash !== "string" || !data.buildHash.trim() || data.buildHash.trim() === "unknown") {
    throw new Error("Playwright preparation refuses the frontend bundle with an unknown identity");
  }
}

export async function fetchHealth(instance = currentDevInstance()): Promise<HealthBody | null> {
  try {
    const response = await fetch(`${instance.apiBase}/api/health`, { signal: AbortSignal.timeout(3000) });
    if (!response.ok) return null;
    return (await response.json()) as HealthBody;
  } catch {
    return null;
  }
}

export async function probeHealth(instance = currentDevInstance()): Promise<HealthVerdict> {
  const expectedSha = testedHeadSha();
  const body = await fetchHealth(instance);
  if (!body) return { ok: false, reason: "status" };
  return evaluateHealth(body, instance, expectedSha);
}

export async function isHealthy(instance = currentDevInstance()): Promise<boolean> {
  return (await probeHealth(instance)).ok;
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
  progress("web-ui readiness", `waiting for ${instance.webUrl} (deadline ${WEB_UI_READY_TIMEOUT_MS}ms)`);
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

async function cdpAppReadiness(instance: DevInstance): Promise<CdpAppReadiness> {
  if (process.platform !== "win32") return { ready: true, matched: instance.webUrl, criterion: "app-origin" };
  const readiness = await probeCdpAppReadiness(instance);
  progress(
    "CDP app readiness",
    readiness.ready
      ? `matched the app page by ${readiness.criterion}: ${readiness.matched}`
      : `not ready: ${readiness.reason}`,
  );
  return readiness;
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
  // build.sh is the only sanctioned entry point (never raw cargo/pnpm tauri).
  execFileSync(BASH_EXE, args, {
    cwd: REPO_ROOT,
    stdio: "inherit",
    timeout: timeoutMs,
    env: buildDevInstanceEnv(instance),
  });
}

export function killDevConsole(instance = currentDevInstance()): void {
  progress("kill", `stopping Playwright instance ${instance.name || "default"}`);
  try {
    bash(instance, ["build.sh", "kill"], 60_000);
  } catch (e) {
    throw new Error(`build.sh kill failed for Playwright instance ${instance.name || "default"}: ${e}`);
  }
  killDevLeftovers(instance);
}

export function runProcessStreaming(
  command: string,
  args: string[],
  options: {
    cwd: string;
    env: NodeJS.ProcessEnv;
    timeoutMs: number;
    phase: string;
    onOutput?: (chunk: string) => void;
  },
): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let timedOut = false;
    const forward = (chunk: Buffer, write: (text: string) => boolean) => {
      const text = chunk.toString();
      write(text);
      options.onOutput?.(text);
    };
    child.stdout.on("data", (chunk: Buffer) => forward(chunk, (text) => process.stdout.write(text)));
    child.stderr.on("data", (chunk: Buffer) => forward(chunk, (text) => process.stderr.write(text)));
    const heartbeat = setInterval(() => {
      progress(options.phase, `still running; child output is streamed live (deadline ${options.timeoutMs}ms)`);
    }, 5_000);
    const deadline = setTimeout(() => {
      timedOut = true;
      progress(options.phase, `deadline ${options.timeoutMs}ms exceeded; terminating child`);
      child.kill();
    }, options.timeoutMs);
    const finish = () => {
      clearInterval(heartbeat);
      clearTimeout(deadline);
    };
    child.once("error", (error) => {
      finish();
      reject(error);
    });
    child.once("close", (code, signal) => {
      finish();
      if (timedOut) {
        reject(new Error(`${command} ${args.join(" ")} timed out after ${options.timeoutMs}ms`));
      } else if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} ${args.join(" ")} exited with ${signal ?? `code ${code}`}`));
      }
    });
  });
}

function bashStreaming(instance: DevInstance, args: string[], timeoutMs: number): Promise<void> {
  return runProcessStreaming(BASH_EXE, args, {
    cwd: REPO_ROOT,
    env: buildDevInstanceEnv(instance),
    timeoutMs,
    phase: "dev --wait",
  });
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
    execFileSync(powershellExe(), windowsCleanupArgs(instance), {
      timeout: 30_000,
      stdio: "inherit",
    });
  } catch (error) {
    throw new Error(`Windows cleanup failed for Playwright instance ${instance.name}: ${error}`);
  }
}

// A PATH without System32 turned every teardown into a phantom process leak.
export function powershellExe(env: NodeJS.ProcessEnv = process.env): string {
  const systemRoot = env.SystemRoot ?? env.SYSTEMROOT ?? env.windir;
  const pinned = systemRoot
    ? path.join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
    : undefined;
  return pinned && fs.existsSync(pinned) ? pinned : "powershell";
}

export function windowsCleanupArgs(instance: DevInstance): string[] {
  const cargoTargetDir = playwrightCargoTargetDir(instance);
  const userProfile = process.env.USERPROFILE ?? process.env.HOME;
  if (!userProfile) throw new Error("Cannot resolve the Playwright instance data directory");
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
    "-DataDir",
    path.join(userProfile, instance.dataDirName),
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
  if (process.env.CT_E2E_REUSE_ONLY === "1") {
    await assertReusableDevConsole(instance);
    progress("adopt", "reusing existing healthy Playwright dev instance without lifecycle changes");
    return;
  }
  const usesWebUi = WEB_MODE || process.platform !== "win32";
  progress("health probe", `checking ${instance.apiBase}/api/health at HEAD ${testedHeadSha()}`);
  const verdict = await probeHealth(instance);
  if (!verdict.ok && verdict.reason === "build") {
    progress("health probe", `${healthMismatchMessage(instance, verdict)}; restarting instead of adopting`);
  }
  if (verdict.ok) {
    progress("health probe", "matching Playwright dev instance is healthy at the tested HEAD");
    if (usesWebUi) {
      progress("web-ui probe", `checking ${instance.webUrl}`);
      if (await waitForWebUi(instance)) {
        progress("adopt", "reusing healthy dev instance via web UI");
        return;
      }
      return await restartUnreadyDevConsole(
        instance,
        usesWebUi,
        `web UI at ${instance.webUrl} never became ready on the healthy instance`,
      );
    }
    progress("CDP probe", `checking ${instance.cdpBase}/json/version`);
    if (await isCdpUp(instance)) {
      progress("CDP app readiness", "checking the WebView2 document");
      const readiness = await cdpAppReadiness(instance);
      if (readiness.ready) {
        progress("adopt", `reusing healthy dev instance via CDP (${readiness.criterion}: ${readiness.matched})`);
        return;
      }
      return await restartUnreadyDevConsole(instance, usesWebUi, readiness.reason);
    }
    throw nonAdoptableCdpError(instance);
  }
  progress("health probe", "no adoptable healthy instance found; preparing a fresh start");
  await startDevConsole(instance, usesWebUi);
}

// Prescribing a restart and then throwing let one bad instance state poison
// every later test; restart for real and carry both reasons if that fails too.
export async function recoverUnreadyInstance(
  instance: DevInstance,
  reason: string,
  restart: () => Promise<void>,
): Promise<void> {
  progress("restart", `healthy instance is not usable (${reason}); restarting through the Playwright harness`);
  try {
    await restart();
  } catch (error) {
    throw new Error(
      `Playwright instance ${instance.name || "default"} was not ready (${reason}) and the harness restart also failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  progress("restart", `restart completed after: ${reason}`);
}

function restartUnreadyDevConsole(instance: DevInstance, usesWebUi: boolean, reason: string): Promise<void> {
  return recoverUnreadyInstance(instance, reason, () => startDevConsole(instance, usesWebUi));
}

async function startDevConsole(instance: DevInstance, usesWebUi: boolean): Promise<void> {
  killDevConsole(instance);
  await assertDevConsoleStopped(instance);
  progress("spawn guard", "all Playwright instance listeners are verified free");
  try {
    progress("dev --wait", "starting build.sh dev --wait (deadline 600000ms)");
    await bashStreaming(instance, devStartArgs(), 600_000);
  } catch (first) {
    dumpBuildGatewayLogs(`first ./build.sh dev --wait failed: ${first}`);
    if ((first as NodeJS.ErrnoException).code === "ETIMEDOUT") throw first;
    // locking the binary, vite squatting 1420) — sweep and retry once.
    killDevLeftovers(instance);
    try {
      await assertDevConsoleStopped(instance);
      progress("dev --wait", "retrying build.sh dev --wait after verified cleanup (deadline 600000ms)");
      await bashStreaming(instance, devStartArgs(), 600_000);
    } catch (second) {
      dumpBuildGatewayLogs(`second ./build.sh dev --wait failed: ${second}`);
      throw second;
    }
  }
  progress("post-start health probe", `checking ${instance.apiBase}/api/health`);
  if (!(await isHealthy(instance))) {
    throw new Error("dev console did not become healthy after ./build.sh dev --wait");
  }
  if (usesWebUi) {
    if (await waitForWebUi(instance)) return;
    throw new Error(
      `dev console is healthy but web UI (${instance.webUrl}) did not become ready`,
    );
  }
  progress("post-start CDP readiness", `waiting for ${instance.cdpBase}`);
  let lastReason = `CDP endpoint ${instance.cdpBase} never answered`;
  for (let i = 0; i < 20; i++) {
    if (await isCdpUp(instance)) {
      const readiness = await cdpAppReadiness(instance);
      if (readiness.ready) return;
      lastReason = readiness.reason;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(
    `dev console is healthy but WebView2 (${instance.cdpBase}) did not finish loading the app: ${lastReason}`,
  );
}

export interface UiSession {
  browser: Browser;
  page: Page;
}

/** A viewport-fixed control that any full-screen modal must cover. Panels that are
 *  `absolute inset-0` inside the main pane (Settings) legitimately do not, so probing
 *  here isolates exactly the overlays that poison a later test. */
export const UI_OVERLAY_PROBE = '[data-testid="titlebar-settings"]';

export type PointerObstruction =
  | { kind: "clear" }
  | { kind: "missing" }
  | { kind: "covered"; by: string };

/**
 * What owns pointer events at the centre of `selector`. "Covered" and "missing" are
 * different bugs that this suite could previously not tell apart — both surfaced only
 * as a long wait — so they are returned as distinct outcomes and never collapsed.
 */
export function pointerObstruction(page: Page, selector: string): Promise<PointerObstruction> {
  return page.evaluate((sel) => {
    const target = document.querySelector(sel);
    if (!(target instanceof HTMLElement)) return { kind: "missing" as const };
    const rect = target.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return { kind: "missing" as const };
    const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    if (!hit) return { kind: "missing" as const };
    if (hit === target || target.contains(hit)) return { kind: "clear" as const };
    const named = hit.closest("[data-testid]");
    return {
      kind: "covered" as const,
      by: named?.getAttribute("data-testid") ?? hit.tagName.toLowerCase(),
    };
  }, selector);
}

async function completeFirstRunPickerIfPresent(page: Page): Promise<void> {
  const picker = page.locator('[data-testid="first-run-general-picker"]');
  if (!(await picker.isVisible({ timeout: 1_000 }).catch(() => false))) return;

  const confirm = page.locator('[data-testid="agent-picker-confirm"]');
  await confirm.waitFor({ state: "visible", timeout: 30_000 });
  if (!(await receivesPointerEvents(confirm))) {
    await activateFirstRunTab(page);
  }
  await expectReceivesPointerEvents(confirm, 30_000);
  await confirm.click({ timeout: 30_000 });
  await picker.waitFor({ state: "detached", timeout: 30_000 });
}

async function activateFirstRunTab(page: Page): Promise<void> {
  // The canvas overlays the tab list. `canvas-toolbar-button` was renamed away and
  // this step has been a silent no-op ever since; CanvasView renders the live close
  // control as `canvas-focus-close`.
  const closeCanvas = page.locator('[data-testid="canvas-focus-close"]');
  if (await closeCanvas.isVisible().catch(() => false)) {
    await closeCanvas.click({ timeout: 30_000 });
  }
  const tab = page
    .locator('[data-testid="tab-item"]')
    .filter({ hasText: "Get started" })
    .first();
  await tab.waitFor({ state: "visible", timeout: 30_000 });
  if (await receivesPointerEvents(tab)) {
    await tab.click({ timeout: 30_000 });
    return;
  }
  await tab.evaluate((node) => {
    if (!(node instanceof HTMLElement)) throw new Error("first-run tab is not an HTMLElement");
    node.click();
  });
}

async function receivesPointerEvents(locator: ReturnType<Page["locator"]>): Promise<boolean> {
  try {
    await locator.click({ trial: true, timeout: 1_000 });
    return true;
  } catch {
    return false;
  }
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

/**
 * The app registers a service worker only when `IS_WEB`
 * (`src/lib/serviceWorkerRegistration.ts`), i.e. when `__TAURI_INTERNALS__` is
 * absent. Asking the page for the product's own condition keeps the desktop
 * CDP path from waiting on a worker that is never registered.
 */
export async function waitForServiceWorkerControl(page: Page): Promise<void> {
  const expectsServiceWorker = await page.evaluate(
    () => !("__TAURI_INTERNALS__" in window) && "serviceWorker" in navigator,
  );
  if (!expectsServiceWorker) return;
  await page.evaluate(
    (deadlineMs) =>
      new Promise<void>((resolve, reject) => {
        const failure = setTimeout(
          () => reject(new Error(`service worker was not ready within ${deadlineMs}ms`)),
          deadlineMs,
        );
        navigator.serviceWorker.ready.then(
          () => {
            clearTimeout(failure);
            resolve();
          },
          (error) => {
            clearTimeout(failure);
            reject(error);
          },
        );
      }),
    SERVICE_WORKER_READY_TIMEOUT_MS,
  );
}

async function waitForShellOnce(page: Page): Promise<void> {
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await waitForServiceWorkerControl(page);
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

export type ResidualProbe = { ok: true; processes: number[] } | { ok: false; failure: string };

/** A probe that could not run names ITSELF; it never borrows "leak" vocabulary. */
export function residualProbeFailure(error: unknown): string {
  return `cannot determine residual processes: ${error instanceof Error ? error.message : String(error)}`;
}

export function parseResidualProbeOutput(output: string): ResidualProbe {
  try {
    const plan = JSON.parse(output) as { selected_processes?: number[] };
    return { ok: true, processes: plan.selected_processes ?? [] };
  } catch (error) {
    return { ok: false, failure: residualProbeFailure(error) };
  }
}

function residualOwnedProcesses(instance: DevInstance): ResidualProbe {
  if (process.platform !== "win32") return { ok: true, processes: [] };
  let output: string;
  try {
    output = execFileSync(powershellExe(), [...windowsCleanupArgs(instance), "-PlanOnly"], {
      encoding: "utf8",
      timeout: 30_000,
    });
  } catch (error) {
    return { ok: false, failure: residualProbeFailure(error) };
  }
  return parseResidualProbeOutput(output);
}

export function stopVerificationFailure(
  instance: DevInstance,
  occupied: number[],
  residual: ResidualProbe,
): string {
  const findings = [
    occupied.length > 0 ? `leaked listeners [${occupied.join(", ")}]` : undefined,
    residual.ok
      ? residual.processes.length > 0
        ? `leaked processes [${residual.processes.join(", ")}]`
        : undefined
      : residual.failure,
  ].filter(Boolean);
  return `Playwright instance ${instance.name || "default"} did not verify as stopped: ${findings.join("; ")}`;
}

export async function assertDevConsoleStopped(
  instance: DevInstance,
  timeoutMs = STOP_VERIFY_TIMEOUT_MS,
): Promise<void> {
  const ports = devInstanceTeardownPorts(instance);
  const deadline = Date.now() + timeoutMs;
  progress("kill verification", `checking listeners [${ports.join(", ")}] (deadline ${timeoutMs}ms)`);
  let nextProgressAt = Date.now() + 3_000;
  let residual = residualOwnedProcesses(instance);
  while (residual.ok && Date.now() <= deadline) {
    if ((await Promise.all(ports.map(portIsFree))).every(Boolean) && residual.processes.length === 0) return;
    if (Date.now() >= nextProgressAt) {
      progress("kill verification", "listeners or owned processes remain; awaiting release");
      nextProgressAt = Date.now() + 3_000;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
    residual = residualOwnedProcesses(instance);
  }
  const occupied = [];
  for (const port of ports) {
    if (!(await portIsFree(port))) occupied.push(port);
  }
  const message = stopVerificationFailure(instance, occupied, residual);
  progress("kill verification", `failed: ${message}`);
  throw new Error(message);
}
