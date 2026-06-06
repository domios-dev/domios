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
import fs from "node:fs";
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

// A bare "bash" resolves to WSL's System32 stub when the runner is launched
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

function bash(args: string[], timeoutMs: number): void {
  // build.sh is the only sanctioned entry point (never raw cargo/pnpm tauri).
  execFileSync(BASH_EXE, args, {
    cwd: REPO_ROOT,
    stdio: "inherit",
    timeout: timeoutMs,
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
      // Harness-only terminal test mode: enables xterm screenReaderMode
      // (the .xterm-accessibility DOM the suites read) and skips the
      // WebGL renderer. User-facing runs never set this — screenReader
      // mode degrades terminal colors (the bad56175 regression).
      VITE_CODETERM_TEST: "1",
      // Debug builds run cron on a MockClock so 07-cron can time-travel
      // via /api/debug/clock. Harmless elsewhere: only cron reads it, and
      // it starts at the real current time.
      CODETERM_TEST_CLOCK: "1",
    },
  });
}

export function killDevConsole(): void {
  bash(["build.sh", "kill"], 60_000);
  // build.sh kill only targets the 7686 listener; its vite child survives
  // holding port 1420 and orphan dev binaries keep the exe locked — both
  // deterministically break the NEXT start, so sweep them every time.
  killDevLeftovers();
}

/**
 * Windows recovery for the recurring crash-leftover states build.sh kill
 * can't see: orphan dev binaries (no 7686 listener but still holding
 * target\debug\codeterm.exe → relink fails with PermissionDenied) and a
 * surviving vite holding port 1420 (next dev start aborts). Scoped hard
 * to the dev target dir / port — prod is untouchable by construction.
 */
function killDevLeftovers(): void {
  if (process.platform !== "win32") return;
  const script = [
    "Get-Process codeterm -ErrorAction SilentlyContinue |",
    "Where-Object { $_.Path -like '*target\\debug*' } |",
    "ForEach-Object { Stop-Process -Id $_.Id -Force -Confirm:$false };",
    "$vite = (netstat -ano | Select-String ':1420.*LISTENING') -replace '.*\\s(\\d+)$','$1' | Select-Object -Unique;",
    "foreach ($p in $vite) { try { Stop-Process -Id $p -Force -Confirm:$false } catch {} }",
  ].join(" ");
  try {
    execFileSync("powershell", ["-NoProfile", "-Command", script], {
      timeout: 30_000,
      stdio: "inherit",
    });
  } catch {
    /* best effort — the dev --wait retry decides success */
  }
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
  try {
    bash(["build.sh", "dev", "--wait"], 600_000);
  } catch {
    // First start attempt can lose to crash leftovers (orphan dev exe
    // locking the binary, vite squatting 1420) — sweep and retry once.
    killDevLeftovers();
    bash(["build.sh", "dev", "--wait"], 600_000);
  }
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
  const browser = await chromium.connectOverCDP(CDP_BASE);
  const pages = browser.contexts().flatMap((c) => c.pages());
  const page =
    pages.find((p) => /localhost|tauri/.test(p.url())) ??
    pages[0] ??
    (() => {
      throw new Error("no page exposed over CDP — dev console window missing?");
    })();
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  // First terminal mounts after session restore + PTY spawn. A pane left in
  // chat-view hides its xterm and shows the composer — both prove the shell
  // is alive.
  await page
    .locator('.xterm:visible, [data-testid="chat-composer"]')
    .first()
    .waitFor({ state: "visible", timeout: 90_000 });
  return { browser, page };
}
