/**
 * 15-extensions — first-class Extensions plugin-manager coverage.
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

function writePlugin(): void {
  const dir = path.join(WORKTREE, "plugins", PLUGIN_ID);
  mkdirSync(path.join(dir, "ui"), { recursive: true });
  writeFileSync(
    path.join(dir, "plugin.json"),
    JSON.stringify(
      {
        id: PLUGIN_ID,
        version: "0.1.0",
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

function setupChannel(): string {
  rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  mkdirSync(CHANNEL_ROOT, { recursive: true });
  bash(`git init --bare ${shQuote(BARE)} >/dev/null`);
  bash(`git clone ${shQuote(BARE)} ${shQuote(WORKTREE)} >/dev/null 2>&1`);
  const pluginsDir = path.join(WORKTREE, "plugins");
  mkdirSync(pluginsDir, { recursive: true });
  writeFileSync(
    path.join(pluginsDir, "channel.json"),
    JSON.stringify(
      {
        schemaVersion: 1,
        name: CHANNEL_NAME,
        plugins: [{ id: PLUGIN_ID, path: PLUGIN_ID, version: "0.1.0" }],
      },
      null,
      2,
    ) + "\n",
  );
  writePlugin();
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

async function openExtensions(ui: ManagedUiSession): Promise<void> {
  await ui.reconnect();
  await ui.page.locator('[data-testid="nav-extensions"]').click();
  await expect(ui.page.locator('[data-testid="extensions-view"]')).toBeVisible();
  await expect(ui.page.locator('[data-testid="extensions-list"]')).toBeVisible();
}

async function addChannel(page: Page, url: string): Promise<void> {
  await page.locator('[data-testid="extensions-channel-add-open"]').click();
  await page.locator('[data-testid="extensions-channel-url-input"]').fill(url);
  await page.locator('[data-testid="extensions-channel-name-input"]').fill(CHANNEL_NAME);
  await page.locator('[data-testid="extensions-channel-add-submit"]').click();
  await expect(
    page.locator(`[data-testid="extensions-section"][data-extension-section="channel:${CHANNEL_NAME}"]`),
  ).toBeVisible({ timeout: 30_000 });
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

test("@smoke list and detail panel render channel extension metadata", async ({ ui }) => {
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

test("@golden installed extension exposes Configure with AI and plugin activity toast", async ({ api, ui }) => {
  await openExtensions(ui);
  await addChannel(ui.page, `${fileUrl(WORKTREE)}#main:plugins`);
  await installPlugin(ui.page, api);

  await expect(
    ui.page.locator(`[data-testid="extensions-configure-ai"][data-plugin-id="${PLUGIN_ID}"]`),
  ).toBeVisible();

  await expect.poll(async () => {
    await api.post("/plugins/view-call", {
      pluginId: PLUGIN_ID,
      method: "progress",
      args: { label: "Downloading fixture payload", percent: 42 },
    });
    return true;
  }).toBe(true);

  const activity = ui.page.locator('[data-testid="extensions-plugin-activity"]').filter({
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

test("@smoke uninstall confirmation includes remove-data checkbox", async ({ api, ui }) => {
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
