/**
 * 05-plugins — plugin channel lifecycle against the real dev console.
 *
 * The suite acts through the rendered Settings → Plugins UI where that is
 * ergonomic, and through the dev CLI for system-channel paths. Every user
 * visible result is asserted against the real Tauri DOM with data-testid
 * selectors, mirroring the 04-mesh pattern.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { connectUi, ensureDevConsole, REPO_ROOT, type UiSession } from "../../helpers/dev-console";
import { devDataDir, wipeDevState } from "../../helpers/reset";

const CHANNEL_NAME = "pw-plugins";
const PLUGIN_ID = "echo";
const CHANNEL_ROOT = path.join(os.tmpdir(), "codeterm-pw-plugin-channel");
const WORKTREE = path.join(CHANNEL_ROOT, "work");
const BARE = path.join(CHANNEL_ROOT, "channel.git");
const BASH = process.platform === "win32" && existsSync("C:\\Program Files\\Git\\bin\\bash.exe")
  ? "C:\\Program Files\\Git\\bin\\bash.exe"
  : "bash";

function bash(script: string): string {
  return execFileSync(BASH, ["-lc", script], {
    cwd: REPO_ROOT,
    encoding: "utf8",
    env: { ...process.env, CODETERM_DATA_DIR: devDataDir() },
  });
}

function shQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

function fileUrl(p: string): string {
  return `file://${p.replace(/\\/g, "/")}`;
}

function writePlugin(id: string, version: string): void {
  const dir = path.join(WORKTREE, "plugins", id);
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    path.join(dir, "plugin.json"),
    JSON.stringify(
      {
        id,
        version,
        displayName: `${id} plugin`,
        spawn: {
          readinessStrategy: { kind: "fixed_delay", durationMs: 1 },
          taskDelivery: "pty_input",
          verifySubmit: { kind: "none" },
          sessionIdInjection: "none",
        },
        models: [{ id: "test", display_name: "Test" }],
        defaultModel: "test",
        entry: "plugin.js",
      },
      null,
      2,
    ) + "\n",
  );
  writeFileSync(path.join(dir, "plugin.js"), "export default {};\n");
}

function writeChannel(version: string): void {
  const pluginsDir = path.join(WORKTREE, "plugins");
  mkdirSync(pluginsDir, { recursive: true });
  writeFileSync(
    path.join(pluginsDir, "channel.json"),
    JSON.stringify(
      {
        schemaVersion: 1,
        name: CHANNEL_NAME,
        plugins: [{ id: PLUGIN_ID, path: PLUGIN_ID, version }],
      },
      null,
      2,
    ) + "\n",
  );
  writePlugin(PLUGIN_ID, version);
}

function setupChannel(version = "0.1.0"): string {
  rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  mkdirSync(CHANNEL_ROOT, { recursive: true });
  bash(`git init --bare ${shQuote(BARE)} >/dev/null`);
  bash(`git clone ${shQuote(BARE)} ${shQuote(WORKTREE)} >/dev/null 2>&1`);
  writeChannel(version);
  bash(
    [
      `git -C ${shQuote(WORKTREE)} checkout -b main >/dev/null 2>&1`,
      `git -C ${shQuote(WORKTREE)} add plugins`,
      `git -C ${shQuote(WORKTREE)} -c user.name=CodeTerm -c user.email=tests@codeterm.local commit -m seed >/dev/null`,
      `git -C ${shQuote(WORKTREE)} push origin main >/dev/null`,
    ].join(" && "),
  );
  return `${fileUrl(WORKTREE)}#main:plugins`;
}

function bumpChannel(version: string): void {
  writeChannel(version);
  bash(
    [
      `git -C ${shQuote(WORKTREE)} add plugins`,
      `git -C ${shQuote(WORKTREE)} -c user.name=CodeTerm -c user.email=tests@codeterm.local commit -m ${shQuote(`bump ${PLUGIN_ID} to ${version}`)} >/dev/null`,
      `git -C ${shQuote(WORKTREE)} push origin main >/dev/null`,
    ].join(" && "),
  );
}

function cli(args: string): string {
  const bin = path.join(devDataDir(), "bin", process.platform === "win32" ? "codeterm.exe" : "codeterm");
  return bash(`${shQuote(bin)} ${args}`);
}

async function openPlugins(): Promise<UiSession> {
  const ui = await connectUi();
  await ui.page.locator('[data-testid="nav-settings"]').click();
  const section = ui.page.locator('[data-testid="settings-section-plugins"]');
  if (await section.isVisible().catch(() => false)) {
    await section.click();
  }
  await expect(ui.page.locator('[data-testid="plugin-installed-list"]')).toBeVisible({
    timeout: 20_000,
  });
  return ui;
}

async function addChannelViaUi(page: Page, url: string): Promise<void> {
  await page.locator('[data-testid="plugin-channel-url-input"]').fill(url);
  await page.locator('[data-testid="plugin-channel-name-input"]').fill(CHANNEL_NAME);
  await page.locator('[data-testid="plugin-add-channel"]').click();
  await expect(
    page.locator(`[data-testid="plugin-channel"][data-channel-name="${CHANNEL_NAME}"]`),
  ).toBeVisible({ timeout: 30_000 });
}

function pluginRow(page: Page, id = PLUGIN_ID) {
  return page.locator(`[data-testid="plugin-row"][data-plugin-id="${id}"]`);
}

function diffRow(page: Page, id = PLUGIN_ID) {
  return page.locator(`[data-testid="plugin-diff"][data-plugin-id="${id}"]`);
}

test.beforeEach(async () => {
  test.setTimeout(180_000);
  wipeDevState();
  setupChannel("0.1.0");
  await ensureDevConsole();
});

test.afterEach(() => {
  if (existsSync(CHANNEL_ROOT)) {
    rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  }
});

test("@golden Scenario A: add channel and install plugin through Settings DOM", async () => {
  const url = `${fileUrl(WORKTREE)}#main:plugins`;
  const ui = await openPlugins();
  try {
    await addChannelViaUi(ui.page, url);
    await expect(diffRow(ui.page)).toHaveAttribute("data-drift", "not_installed", {
      timeout: 30_000,
    });
    await ui.page.locator(`[data-testid="plugin-install-${PLUGIN_ID}"]`).click();
    await expect(pluginRow(ui.page)).toHaveAttribute("data-version", "0.1.0", {
      timeout: 30_000,
    });
    await expect(pluginRow(ui.page)).toHaveAttribute("data-source-kind", "channel");
    await expect(pluginRow(ui.page)).toHaveAttribute("data-source-channel", CHANNEL_NAME);
    await expect(pluginRow(ui.page)).toHaveAttribute("data-update-available", "false");
  } finally {
    await ui.browser.close().catch(() => {});
  }
});

test("@golden Scenario B: channel bump renders update badge, click update clears it", async () => {
  const url = `${fileUrl(WORKTREE)}#main:plugins`;
  const ui = await openPlugins();
  try {
    await addChannelViaUi(ui.page, url);
    await ui.page.locator(`[data-testid="plugin-install-${PLUGIN_ID}"]`).click();
    await expect(pluginRow(ui.page)).toHaveAttribute("data-version", "0.1.0", {
      timeout: 30_000,
    });

    bumpChannel("0.1.1");
    await ui.page.locator(`[data-testid="plugin-channel-refresh-${CHANNEL_NAME}"]`).click();
    await expect(pluginRow(ui.page)).toHaveAttribute("data-update-available", "true", {
      timeout: 30_000,
    });
    await expect(pluginRow(ui.page).locator('[data-testid="plugin-update-available-badge"]')).toBeVisible();

    await ui.page.locator(`[data-testid="plugin-update-${PLUGIN_ID}"]`).click();
    await expect(pluginRow(ui.page)).toHaveAttribute("data-version", "0.1.1", {
      timeout: 30_000,
    });
    await expect(pluginRow(ui.page)).toHaveAttribute("data-update-available", "false");
  } finally {
    await ui.browser.close().catch(() => {});
  }
});

test("@golden Scenario C: system channel update renders bundled plugins from develop", async () => {
  cli("plugin update-system --json");
  const ui = await openPlugins();
  try {
    await expect(
      ui.page.locator('[data-testid="plugin-channel"][data-channel-name="codeterm-core"]'),
    ).toBeVisible({ timeout: 30_000 });
    for (const id of ["claude", "codex", "opencode"]) {
      await expect(pluginRow(ui.page, id)).toHaveAttribute("data-version", "0.1.0", {
        timeout: 30_000,
      });
      await expect(pluginRow(ui.page, id)).toHaveAttribute("data-source-channel", "codeterm-core");
      await expect(pluginRow(ui.page, id)).toHaveAttribute("data-verified", "true");
    }
  } finally {
    await ui.browser.close().catch(() => {});
  }
});

test("@smoke @failure Scenario D: plugin failures are rendered in Settings DOM", async () => {
  const brokenChannel = `broken-${Date.now()}`;
  const ui = await openPlugins();
  try {
    await ui.page.locator('[data-testid="plugin-channel-url-input"]').fill(`${fileUrl(CHANNEL_ROOT)}/missing#main:plugins`);
    await ui.page.locator('[data-testid="plugin-channel-name-input"]').fill(brokenChannel);
    await ui.page.locator('[data-testid="plugin-add-channel"]').click();
    await expect(ui.page.locator('[data-testid="plugin-manager-error"]')).toBeVisible({
      timeout: 30_000,
    });
    const ownErrorToast = ui.page
      .locator('[data-testid="plugin-error-toast"]')
      .filter({ hasText: brokenChannel });
    await expect(ownErrorToast).toHaveCount(1, {
      timeout: 30_000,
    });
    await expect(ownErrorToast).toBeVisible();
  } finally {
    await ui.browser.close().catch(() => {});
  }
});
