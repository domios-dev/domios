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
import { readDevToken } from "./api-client";

export const E2E_PORT = process.env.CT_E2E_PORT ?? "7686";
export const API_BASE = `http://127.0.0.1:${E2E_PORT}`;
export const CDP_PORT = 9223;
export const CDP_BASE = `http://127.0.0.1:${CDP_PORT}`;
export const WEB_URL = `${API_BASE}/`;

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
      VITE_CODETERM_TEST: "1",
      CODETERM_TEST_CLOCK: "1",
    },
  });
}

export function killDevConsole(): void {
  // A failing kill must not abort setup: on a fresh/CI runner there is nothing
  // to kill, and Windows process enumeration can EPERM on the protected
  // %USERPROFILE%\AppData junction. killDevLeftovers + `dev --wait` recover.
  try {
    bash(["build.sh", "kill"], 60_000);
  } catch (e) {
    console.warn(`[dev-console] build.sh kill failed (continuing): ${e}`);
  }
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
  const root = REPO_ROOT.replace(/'/g, "''");
  const script = `
    $ErrorActionPreference = "SilentlyContinue"
    $rootPath = (Resolve-Path -LiteralPath '${root}').Path
    $targetDebug = Join-Path $rootPath "src-tauri\\target\\debug"
    $nodeModules = Join-Path $rootPath "node_modules"
    $selfPid = $PID

    $all = @(Get-CimInstance Win32_Process)
    $childrenByParent = @{}
    foreach ($proc in $all) {
      $parent = [int]$proc.ParentProcessId
      if (-not $childrenByParent.ContainsKey($parent)) {
        $childrenByParent[$parent] = New-Object System.Collections.Generic.List[object]
      }
      $childrenByParent[$parent].Add($proc)
    }

    function Stop-Tree {
      param([int]$ProcessId, [hashtable]$Seen)
      if ($ProcessId -le 0 -or $Seen.ContainsKey($ProcessId)) { return }
      $Seen[$ProcessId] = $true
      if ($childrenByParent.ContainsKey($ProcessId)) {
        foreach ($child in @($childrenByParent[$ProcessId])) {
          Stop-Tree -ProcessId ([int]$child.ProcessId) -Seen $Seen
        }
      }
      try { Stop-Process -Id $ProcessId -Force -Confirm:$false -ErrorAction Stop } catch {}
    }

    $victims = New-Object System.Collections.Generic.HashSet[int]
    foreach ($proc in $all) {
      $procId = [int]$proc.ProcessId
      if ($procId -eq $selfPid) { continue }
      $cmd = [string]$proc.CommandLine
      $exe = [string]$proc.ExecutablePath
      $name = [string]$proc.Name
      $isRepoScriptHost = @("node.exe", "npm.exe", "pnpm.exe", "cmd.exe", "sh.exe", "bash.exe") -contains $name.ToLowerInvariant()
      $matchesTarget = ($exe -and $exe.StartsWith($targetDebug, [System.StringComparison]::OrdinalIgnoreCase)) -or
        ($cmd -and $cmd.IndexOf($targetDebug, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
      $matchesDevWebView = $cmd -and $cmd.IndexOf("com.codeterm.dev\\EBWebView", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
      $matchesRepoVite = $isRepoScriptHost -and $cmd -and
        $cmd.IndexOf($nodeModules, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -and
        ($cmd -match "vite\\\\bin\\\\vite\\.js|@tauri-apps\\\\cli\\\\tauri\\.js")

      if ($matchesTarget -or $matchesDevWebView -or $matchesRepoVite) {
        [void]$victims.Add($procId)
      }
    }

    $seen = @{}
    foreach ($procId in @($victims)) {
      Stop-Tree -ProcessId $procId -Seen $seen
    }

    $vite = (netstat -ano | Select-String ':1420.*LISTENING') -replace '.*\\s(\\d+)$','$1' | Select-Object -Unique
    foreach ($p in $vite) {
      try { taskkill /PID $p /T /F | Out-Null } catch {}
    }
  `;
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
  if (await isHealthy()) {
    if (process.platform !== "win32" || (await isCdpUp())) return;
  }
  killDevConsole();
  try {
    bash(["build.sh", "dev", "--wait"], 600_000);
  } catch {
    // locking the binary, vite squatting 1420) — sweep and retry once.
    killDevLeftovers();
    bash(["build.sh", "dev", "--wait"], 600_000);
  }
  if (!(await isHealthy())) {
    throw new Error("dev console did not become healthy after ./build.sh dev --wait");
  }
  if (process.platform !== "win32") return;
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

async function completeFirstRunPickerIfPresent(page: Page): Promise<void> {
  const picker = page.locator('[data-testid="first-run-general-picker"]');
  if (!(await picker.isVisible({ timeout: 1_000 }).catch(() => false))) return;

  const confirm = page.locator('[data-testid="agent-picker-confirm"]');
  await confirm.waitFor({ state: "visible", timeout: 30_000 });
  await confirm.click({ timeout: 30_000 });
  await picker.waitFor({ state: "detached", timeout: 30_000 });
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

async function connectDesktopUi(): Promise<UiSession> {
  const browser = await chromium.connectOverCDP(CDP_BASE);
  const pages = browser.contexts().flatMap((c) => c.pages()).filter((p) => !p.isClosed());
  const page =
    pages.find((p) => /localhost|tauri/.test(p.url())) ??
    pages[0] ??
    (() => {
      throw new Error("no page exposed over CDP — dev console window missing?");
    })();
  await waitForShell(page);
  return { browser, page };
}

async function connectWebUi(): Promise<UiSession> {
  const token = readDevToken();
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
  await page.goto(`${WEB_URL}#token=${encodeURIComponent(token)}`, { waitUntil: "domcontentloaded" });
  await waitForShell(page);
  return { browser, page };
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
      const session = process.platform === "win32" ? await connectDesktopUi() : await connectWebUi();
      browser = session.browser;
      return session;
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
