/**
 * Settings -> Plugins GUI lifecycle through Tauri WebDriver.
 *
 * This is the macOS-compatible counterpart to tests/playwright/suites/05-plugins:
 * Playwright drives Windows WebView2 over CDP, while this spec drives the same
 * rendered DOM through the app's WebDriver plugin (WKWebView works here).
 */

import { execFileSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { api } from '../helpers.js';

const CHANNEL_NAME = 'wdio-plugins';
const PLUGIN_ID = 'echo';
const CHANNEL_ROOT = path.join(os.tmpdir(), 'codeterm-wdio-plugin-channel');
const WORKTREE = path.join(CHANNEL_ROOT, 'work');
const BARE = path.join(CHANNEL_ROOT, 'channel.git');
const DEV_DIR = path.join(os.homedir(), '.codeterm-dev');
const CLI = path.join(DEV_DIR, 'bin', process.platform === 'win32' ? 'codeterm.exe' : 'codeterm');
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');

function shQuote(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function run(script) {
  return execFileSync('bash', ['-lc', script], {
    cwd: path.resolve(import.meta.dirname, '..', '..', '..'),
    encoding: 'utf8',
    env: { ...process.env, CODETERM_DATA_DIR: DEV_DIR },
  });
}

function fileUrl(p) {
  return `file://${p.replace(/\\/g, '/')}`;
}

function writePlugin(version) {
  const dir = path.join(WORKTREE, 'plugins', PLUGIN_ID);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(
    path.join(dir, 'plugin.json'),
    JSON.stringify({
      id: PLUGIN_ID,
      version,
      displayName: 'echo plugin',
      spawn: {
        readinessStrategy: { kind: 'fixed_delay', durationMs: 1 },
        taskDelivery: 'pty_input',
        verifySubmit: { kind: 'none' },
        sessionIdInjection: 'none',
      },
      models: [{ id: 'test', display_name: 'Test' }],
      defaultModel: 'test',
      entry: 'plugin.js',
    }, null, 2) + '\n',
  );
  fs.writeFileSync(path.join(dir, 'plugin.js'), 'export default {};\n');
}

function writeChannel(version) {
  fs.mkdirSync(path.join(WORKTREE, 'plugins'), { recursive: true });
  fs.writeFileSync(
    path.join(WORKTREE, 'plugins', 'channel.json'),
    JSON.stringify({
      schemaVersion: 1,
      name: CHANNEL_NAME,
      plugins: [{ id: PLUGIN_ID, path: PLUGIN_ID, version }],
    }, null, 2) + '\n',
  );
  writePlugin(version);
}

function setupChannel(version = '0.1.0') {
  fs.rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  fs.mkdirSync(CHANNEL_ROOT, { recursive: true });
  run(`git init --bare ${shQuote(BARE)} >/dev/null`);
  run(`git clone ${shQuote(BARE)} ${shQuote(WORKTREE)} >/dev/null 2>&1`);
  writeChannel(version);
  run([
    `git -C ${shQuote(WORKTREE)} checkout -b main >/dev/null 2>&1`,
    `git -C ${shQuote(WORKTREE)} add plugins`,
    `git -C ${shQuote(WORKTREE)} -c user.name=CodeTerm -c user.email=tests@codeterm.local commit -m seed >/dev/null`,
    `git -C ${shQuote(WORKTREE)} push origin main >/dev/null`,
  ].join(' && '));
}

function bumpChannel(version) {
  writeChannel(version);
  run([
    `git -C ${shQuote(WORKTREE)} add plugins`,
    `git -C ${shQuote(WORKTREE)} -c user.name=CodeTerm -c user.email=tests@codeterm.local commit -m ${shQuote(`bump ${PLUGIN_ID} to ${version}`)} >/dev/null`,
    `git -C ${shQuote(WORKTREE)} push origin main >/dev/null`,
  ].join(' && '));
}

function wipePluginState() {
  for (const entry of ['plugins', 'channels', 'plugin-channels.json', 'plugin-audit.jsonl']) {
    fs.rmSync(path.join(DEV_DIR, entry), { recursive: true, force: true });
  }
}

function cli(args) {
  return run(`${shQuote(CLI)} ${args}`);
}

async function waitForSelector(selector, timeout = 30000) {
  await browser.waitUntil(async () => browser.execute((s) => !!document.querySelector(s), selector), {
    timeout,
    interval: 500,
    timeoutMsg: `selector did not appear: ${selector}`,
  });
}

async function attr(selector, name) {
  return browser.execute((s, n) => document.querySelector(s)?.getAttribute(n) ?? null, selector, name);
}

async function setValue(selector, value) {
  const el = await $(selector);
  await el.waitForExist({ timeout: 10000 });
  await el.setValue(value);
}

async function click(selector) {
  const el = await $(selector);
  await el.waitForExist({ timeout: 10000 });
  await el.click();
}

async function openPlugins() {
  await waitForSelector('[data-testid="nav-settings"]', 60000);
  await click('[data-testid="nav-settings"]');
  await browser.pause(500);
  const hasSection = await browser.execute(() => !!document.querySelector('[data-testid="settings-section-plugins"]'));
  if (hasSection) {
    await click('[data-testid="settings-section-plugins"]');
  }
  await waitForSelector('[data-testid="plugin-installed-list"]', 30000);
  if (await browser.execute(() => !!document.querySelector('[data-testid="plugin-refresh"]'))) {
    await click('[data-testid="plugin-refresh"]');
    await browser.pause(500);
  }
}

async function addChannel(url) {
  await setValue('[data-testid="plugin-channel-url-input"]', url);
  await setValue('[data-testid="plugin-channel-name-input"]', CHANNEL_NAME);
  await click('[data-testid="plugin-add-channel"]');
  await waitForSelector(`[data-testid="plugin-channel"][data-channel-name="${CHANNEL_NAME}"]`);
}

function pluginRow(id = PLUGIN_ID) {
  return `[data-testid="plugin-row"][data-plugin-id="${id}"]`;
}

function diffRow(id = PLUGIN_ID) {
  return `[data-testid="plugin-diff"][data-plugin-id="${id}"]`;
}

describe('Settings Plugins lifecycle', () => {
  beforeEach(async () => {
    wipePluginState();
    setupChannel();
    await api('GET', '/health');
    await browser.pause(1000);
  });

  afterEach(async () => {
    fs.rmSync(CHANNEL_ROOT, { recursive: true, force: true });
  });

  it('Scenario A: add channel and install plugin through Settings DOM', async () => {
    await openPlugins();
    await addChannel(`${fileUrl(WORKTREE)}#main:plugins`);
    await browser.waitUntil(async () => await attr(diffRow(), 'data-drift') === 'not_installed', {
      timeout: 30000,
      timeoutMsg: 'echo diff did not become not_installed',
    });
    await click(`[data-testid="plugin-install-${PLUGIN_ID}"]`);
    await browser.waitUntil(async () => await attr(pluginRow(), 'data-version') === '0.1.0', {
      timeout: 30000,
      timeoutMsg: 'echo plugin did not install at 0.1.0',
    });
    expect(await attr(pluginRow(), 'data-source-kind')).toBe('channel');
    expect(await attr(pluginRow(), 'data-source-channel')).toBe(CHANNEL_NAME);
  });

  it('Scenario B: channel bump renders update badge and update clears it', async () => {
    await openPlugins();
    await addChannel(`${fileUrl(WORKTREE)}#main:plugins`);
    await click(`[data-testid="plugin-install-${PLUGIN_ID}"]`);
    await browser.waitUntil(async () => await attr(pluginRow(), 'data-version') === '0.1.0', {
      timeout: 30000,
      timeoutMsg: 'echo plugin did not install before bump',
    });

    bumpChannel('0.1.1');
    await click(`[data-testid="plugin-channel-refresh-${CHANNEL_NAME}"]`);
    await browser.waitUntil(async () => await attr(pluginRow(), 'data-update-available') === 'true', {
      timeout: 30000,
      timeoutMsg: 'update badge did not render',
    });
    await waitForSelector(`${pluginRow()} [data-testid="plugin-update-available-badge"]`);
    await click(`[data-testid="plugin-update-${PLUGIN_ID}"]`);
    await browser.waitUntil(async () => await attr(pluginRow(), 'data-version') === '0.1.1', {
      timeout: 30000,
      timeoutMsg: 'echo plugin did not update to 0.1.1',
    });
    expect(await attr(pluginRow(), 'data-update-available')).toBe('false');
  });

  it('Scenario C: system channel update renders bundled plugins', async () => {
    run(
      `CODETERM_SYSTEM_PLUGIN_CHANNEL_URL=${shQuote(fileUrl(path.join(REPO_ROOT, 'plugins')))} ` +
        `${shQuote(CLI)} plugin update-system --json`,
    );
    await openPlugins();
    await waitForSelector('[data-testid="plugin-channel"][data-channel-name="codeterm-core"]');
    for (const id of ['claude', 'codex', 'opencode']) {
      await waitForSelector(pluginRow(id));
      expect(await attr(pluginRow(id), 'data-version')).toBe('0.1.0');
      expect(await attr(pluginRow(id), 'data-source-channel')).toBe('codeterm-core');
      expect(await attr(pluginRow(id), 'data-verified')).toBe('true');
    }
  });

  it('Scenario D: bad channel errors render in Settings DOM', async () => {
    await openPlugins();
    await setValue('[data-testid="plugin-channel-url-input"]', `${fileUrl(CHANNEL_ROOT)}/missing#main:plugins`);
    await setValue('[data-testid="plugin-channel-name-input"]', 'broken');
    await click('[data-testid="plugin-add-channel"]');
    await waitForSelector('[data-testid="plugin-manager-error"]');
    await waitForSelector('[data-testid="plugin-error-toast"]');
  });
});
