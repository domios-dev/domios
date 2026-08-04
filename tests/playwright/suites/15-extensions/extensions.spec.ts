/**
 * WHAT: extensions covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 15-extensions.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, test, type ManagedUiSession, type Page } from "../../helpers/fixtures";
import { ensureDevConsole, REPO_ROOT } from "../../helpers/dev-console";
import { devDataDir, wipeDevState } from "../../helpers/reset";

type PluginSnapshot = {
  plugins: Array<{ id: string; version?: string | null }>;
};

type PluginApi = {
  get<T>(p: string): Promise<T>;
  post<T = unknown>(p: string, body?: unknown): Promise<T>;
};

const CHANNEL_NAME = "pw-track-c-extensions";
const PLUGIN_ID = "pw-track-c-view";
const CHANNEL_ROOT = path.join(os.tmpdir(), "codeterm-pw-track-c-extensions");
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

function writePlugin(version = "0.1.0"): void {
  const dir = path.join(WORKTREE, "plugins", PLUGIN_ID);
  mkdirSync(path.join(dir, "ui"), { recursive: true });
  writeFileSync(
    path.join(dir, "plugin.json"),
    JSON.stringify(
      {
        id: PLUGIN_ID,
        version,
        minCodeterm: "1.0.0",
        hostApi: 2,
        displayName: "PW Track C View",
        entry: "plugin.js",
        capabilities: { view: true },
        permissions: {},
        description: "Playwright fixture extension for list, detail, activity, and uninstall coverage.",
        configHelp: "Help configure the Playwright Track C fixture extension.",
      },
      null,
      2,
    ) + "\n",
  );
  writeFileSync(
    path.join(dir, "plugin.js"),
    [
      "function viewCall(method, args) {",
      "  if (method === 'progress') {",
      "    host.progress({ label: args.label, percent: args.percent });",
      "    return { ok: true };",
      "  }",
      "  if (method === 'finish') {",
      "    host.progress({ label: args.label || 'Done', percent: 100, done: true });",
      "    return { ok: true };",
      "  }",
      "  return { error: 'unknown method: ' + method };",
      "}",
      "module.exports.default = { viewCall: viewCall };",
      "",
    ].join("\n"),
  );
  writeFileSync(path.join(dir, "ui", "index.html"), "<!doctype html><title>PW Track C View</title>\n");
}

function writeChannelManifest(version: string): void {
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
  writePlugin(version);
}

function setupChannel(): string {
  rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  mkdirSync(CHANNEL_ROOT, { recursive: true });
  bash(`git init --bare ${shQuote(BARE)} >/dev/null`);
  bash(`git clone ${shQuote(BARE)} ${shQuote(WORKTREE)} >/dev/null 2>&1`);
  writeChannelManifest("0.1.0");
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
  writeChannelManifest(version);
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

async function openExtensions(ui: ManagedUiSession): Promise<void> {
  await ui.reconnect();
  await ui.page.locator('[data-testid="nav-extensions"]').click();
  await expect(ui.page.locator('[data-testid="extensions-view"]')).toBeVisible();
  await expect(ui.page.locator('[data-testid="extensions-list"]')).toBeVisible();
}

async function addChannel(page: Page, url: string): Promise<void> {
  // AddChannelInline only mounts in the Channels filter's detail tab (ExtensionsView.tsx).
  await page.locator('[data-testid="extensions-filter"][data-extension-filter="channels"]').click();
  await page.locator('[data-testid="extensions-channel-add-open"]').click();
  await page.locator('[data-testid="extensions-channel-url-input"]').fill(url);
  await page.locator('[data-testid="extensions-channel-name-input"]').fill(CHANNEL_NAME);
  await page.locator('[data-testid="extensions-channel-add-submit"]').click();
  // extensions-section (source-grouped rows) only renders outside the Channels filter.
  await page.locator('[data-testid="extensions-filter"][data-extension-filter="all"]').click();
  const section = page.locator(
    `[data-testid="extensions-section"][data-extension-section="channel:${CHANNEL_NAME}"]`,
  );
  // The channel marketplace diff refreshes on a background thread (diff_cache.rs) and
  // isn't re-polled by the UI on its own; nudge extensions-refresh until it lands.
  await expect
    .poll(
      async () => {
        if (await section.isVisible().catch(() => false)) return true;
        await page.locator('[data-testid="extensions-refresh"]').click().catch(() => {});
        return false;
      },
      { timeout: 30_000, intervals: [1_000] },
    )
    .toBe(true);
}

function pluginRow(page: Page) {
  return page.locator(`[data-testid="extensions-plugin-row"][data-plugin-id="${PLUGIN_ID}"]`);
}

async function selectPlugin(page: Page): Promise<void> {
  await pluginRow(page).click();
  await expect(
    page.locator(`[data-testid="extensions-plugin-detail"][data-plugin-id="${PLUGIN_ID}"]`),
  ).toBeVisible();
}

async function installPlugin(page: Page, api: PluginApi): Promise<void> {
  await selectPlugin(page);
  await page.locator(`[data-testid="extensions-plugin-install"][data-plugin-id="${PLUGIN_ID}"]`).click();
  await expect.poll(async () => {
    const snapshot = await api.get<PluginSnapshot>("/plugins/snapshot");
    return snapshot.plugins.some((plugin) => plugin.id === PLUGIN_ID);
  }).toBe(true);
  await expect(pluginRow(page)).toHaveAttribute("data-plugin-installed", "true", { timeout: 30_000 });
  await selectPlugin(page);
}

test.beforeEach(async () => {
  test.setTimeout(180_000);
  wipeDevState();
  setupChannel();
  await ensureDevConsole();
});

test.afterEach(() => {
  rmSync(CHANNEL_ROOT, { recursive: true, force: true });
});

test("@smoke list and detail panel render channel extension metadata @restart", async ({ ui }) => {
  await openExtensions(ui);
  await addChannel(ui.page, `${fileUrl(WORKTREE)}#main:plugins`);
  await expect(pluginRow(ui.page)).toBeVisible({ timeout: 30_000 });

  await selectPlugin(ui.page);
  const detail = ui.page.locator(`[data-testid="extensions-plugin-detail"][data-plugin-id="${PLUGIN_ID}"]`);
  await expect(detail).toContainText("PW Track C View");
  await expect(detail.locator('[data-testid="extensions-description"]')).toContainText(
    "Playwright fixture extension",
  );
  await expect(detail.locator('[data-testid="extensions-capabilities"]')).toContainText("view");
});

test("@golden installed extension exposes Configure with AI and plugin activity toast @restart", async ({ api, ui }) => {
  await openExtensions(ui);
  await addChannel(ui.page, `${fileUrl(WORKTREE)}#main:plugins`);
  await installPlugin(ui.page, api);

  await expect(
    ui.page.locator(`[data-testid="extensions-configure-ai"][data-plugin-id="${PLUGIN_ID}"]`),
  ).toBeVisible();

  await expect.poll(async () => {
    try {
      await api.post("/plugins/view-call", {
        pluginId: PLUGIN_ID,
        method: "progress",
        args: { label: "Downloading fixture payload", percent: 42 },
      });
      return true;
    } catch (error) {
      if (String(error).includes("no view provider")) return false;
      throw error;
    }
  }, { timeout: 30_000, intervals: [250, 500, 1_000] }).toBe(true);

  const activity = ui.page
    .locator(`[data-testid="extensions-plugin-detail"][data-plugin-id="${PLUGIN_ID}"]`)
    .locator('[data-testid="extensions-plugin-activity"]')
    .filter({
      hasText: "Downloading fixture payload",
    });
  await expect(activity).toBeVisible({ timeout: 20_000 });
  await expect(activity.locator('[data-testid="extensions-plugin-activity-progress"]')).toHaveAttribute(
    "aria-label",
    "42%",
  );
  const toast = ui.page.locator(
    `[data-testid="notification-toast"][data-notification-key="plugin-activity:${PLUGIN_ID}"]`,
  );
  await expect(toast).toBeVisible({ timeout: 20_000 });
  await expect(toast).toContainText("Downloading fixture payload");

  await api.post("/plugins/view-call", {
    pluginId: PLUGIN_ID,
    method: "finish",
    args: { label: "Finished fixture payload" },
  });
  await expect(toast).toHaveCount(0);
});

test("@smoke uninstall confirmation includes remove-data checkbox @restart", async ({ api, ui }) => {
  await openExtensions(ui);
  await addChannel(ui.page, `${fileUrl(WORKTREE)}#main:plugins`);
  await installPlugin(ui.page, api);

  await ui.page.locator(`[data-testid="extensions-plugin-uninstall"][data-plugin-id="${PLUGIN_ID}"]`).click();
  const dialog = ui.page.locator(`[data-testid="extensions-uninstall-confirm"][data-plugin-id="${PLUGIN_ID}"]`);
  await expect(dialog).toBeVisible();
  const removeData = dialog.locator('[data-testid="extensions-uninstall-remove-data"]');
  await expect(removeData).toBeChecked();

  await removeData.uncheck();
  await expect(removeData).not.toBeChecked();
  await removeData.check();
  await expect(removeData).toBeChecked();

  await dialog.locator('[data-testid="extensions-uninstall-confirm-submit"]').click();
  await expect.poll(async () => {
    const snapshot = await api.get<PluginSnapshot>("/plugins/snapshot");
    return snapshot.plugins.some((plugin) => plugin.id === PLUGIN_ID);
  }).toBe(false);
});

test("@golden channel bump renders update badge; update installs the new version @restart", async ({ api, ui }) => {
  await openExtensions(ui);
  await addChannel(ui.page, `${fileUrl(WORKTREE)}#main:plugins`);
  await installPlugin(ui.page, api);
  const row = pluginRow(ui.page);
  await expect(row).toHaveAttribute("data-plugin-version", "0.1.0", { timeout: 30_000 });
  await expect(row).toHaveAttribute("data-plugin-channel", CHANNEL_NAME);
  await expect(row).toHaveAttribute("data-plugin-update-available", "false");

  bumpChannel("0.1.1");
  // Channel diff refreshes on a background thread; nudge extensions-refresh until the flag lands.
  await expect
    .poll(
      async () => {
        if ((await row.getAttribute("data-plugin-update-available")) === "true") return true;
        await ui.page.locator('[data-testid="extensions-refresh"]').click().catch(() => {});
        return false;
      },
      { timeout: 30_000, intervals: [1_000] },
    )
    .toBe(true);

  await selectPlugin(ui.page);
  await ui.page.locator(`[data-testid="extensions-plugin-update"][data-plugin-id="${PLUGIN_ID}"]`).click();
  await expect(row).toHaveAttribute("data-plugin-version", "0.1.1", { timeout: 30_000 });
  await expect(row).toHaveAttribute("data-plugin-update-available", "false", { timeout: 30_000 });
});

test("@golden system channel update renders bundled providers as verified @restart", async ({ ui }) => {
  cli("plugin update-system --json");
  await openExtensions(ui);
  const section = ui.page.locator(
    '[data-testid="extensions-section"][data-extension-section="channel:codeterm-core"]',
  );
  await expect
    .poll(
      async () => {
        if (await section.isVisible().catch(() => false)) return true;
        await ui.page.locator('[data-testid="extensions-refresh"]').click().catch(() => {});
        return false;
      },
      { timeout: 30_000, intervals: [1_000] },
    )
    .toBe(true);
  for (const id of ["claude", "codex", "opencode"]) {
    const row = ui.page.locator(`[data-testid="extensions-plugin-row"][data-plugin-id="${id}"]`);
    await expect(row).toHaveAttribute("data-plugin-installed", "true", { timeout: 30_000 });
    await expect(row).toHaveAttribute("data-plugin-channel", "codeterm-core");
    await expect(row).toHaveAttribute("data-plugin-verified", "true");
  }
});

test("@smoke broken channel add surfaces the error banner @restart", async ({ ui }) => {
  await openExtensions(ui);
  await ui.page.locator('[data-testid="extensions-filter"][data-extension-filter="channels"]').click();
  await ui.page.locator('[data-testid="extensions-channel-add-open"]').click();
  await ui.page
    .locator('[data-testid="extensions-channel-url-input"]')
    .fill(`${fileUrl(CHANNEL_ROOT)}/missing#main:plugins`);
  await ui.page.locator('[data-testid="extensions-channel-name-input"]').fill(`broken-${Date.now()}`);
  await ui.page.locator('[data-testid="extensions-channel-add-submit"]').click();
  const error = ui.page.locator('[data-testid="extensions-error"]');
  await expect(error).toBeVisible({ timeout: 30_000 });
  await expect(error).toContainText(/missing|error|failed|not found/i);
  // Leave the live dev window clean: the deliberately-broken add is asserted, drop its banner.
  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
});
