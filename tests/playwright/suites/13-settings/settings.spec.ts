/**
 * 13-settings — Settings UI coverage for stable General-tab surfaces.
 * Excludes the model picker while that work is in flight on master.
 */
import type { AppConfig } from "../../../../src/lib/types";
import { expect, test, type ManagedUiSession } from "../../helpers/fixtures";

async function openSettings(ui: ManagedUiSession): Promise<void> {
  await ui.reconnect();
  await ui.page.locator('[data-testid="nav-settings"]').click();
  await expect(ui.page.locator('[data-testid="settings-panel"]')).toBeVisible();
  const section = ui.page.locator('[data-testid="settings-section-general"]');
  if (await section.isVisible().catch(() => false)) {
    await section.click();
  }
  await expect(ui.page.locator('[data-testid="settings-theme-select"]')).toBeVisible();
}

async function restoreConfig(api: { request<T>(method: string, apiPath: string, body?: unknown): Promise<T> }, config: AppConfig): Promise<void> {
  await api.request<AppConfig>("PATCH", "/config", config);
}

function expectedShortcutText(binding: string): string {
  const isMac = process.platform === "darwin";
  return binding
    .split("+")
    .map((part, _index, parts) => {
      const key = part.trim().toLowerCase();
      if (isMac) {
        if (key === "ctrl" && parts.some((p) => /^[a-zA-Z0-9]$/.test(p.trim()))) return "⌘";
        if (key === "ctrl") return "⌃";
        if (key === "shift") return "⇧";
        if (key === "alt") return "⌥";
        if (key === "cmd" || key === "mod" || key === "super") return "⌘";
      } else {
        if (key === "cmd" || key === "mod") return "Ctrl";
        if (key === "super") return "Win";
      }
      return part.trim().toUpperCase();
    })
    .join("");
}

test("@smoke theme selection applies to live config and CSS variables", async ({ api, ui }) => {
  const original = await api.get<AppConfig>("/config");
  try {
    await openSettings(ui);
    const select = ui.page.locator('[data-testid="settings-theme-select"]');
    const options = await select.locator("option").evaluateAll((nodes) =>
      nodes.map((node) => (node as HTMLOptionElement).value),
    );
    const target = options.find((value) => value !== original.theme.name);
    expect(target, `theme options: ${options.join(", ")}`).toBeTruthy();

    const beforeBg = await ui.page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--ct-bg").trim(),
    );
    await select.selectOption(target!);

    await expect(select).toHaveValue(target!);
    await expect.poll(async () => (await api.get<AppConfig>("/config")).theme.name).toBe(target);
    const afterBg = await ui.page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--ct-bg").trim(),
    );
    expect(afterBg).toBeTruthy();
    expect(afterBg).not.toBe(beforeBg);
  } finally {
    await restoreConfig(api, original);
  }
});

test("@smoke font family edit persists through the Settings UI", async ({ api, ui }) => {
  const original = await api.get<AppConfig>("/config");
  const nextFamily = `JetBrains Mono, pw-settings-${Date.now()}, monospace`;
  try {
    await openSettings(ui);
    const input = ui.page.locator('[data-testid="settings-font-family-input"]');

    await input.fill(nextFamily);
    await expect(input).toHaveValue(nextFamily);
    await expect.poll(async () => (await api.get<AppConfig>("/config")).font.family).toBe(nextFamily);
  } finally {
    await restoreConfig(api, original);
  }
});

test("@smoke keybinding display renders configured shortcuts", async ({ api, ui }) => {
  const config = await api.get<AppConfig>("/config");
  await openSettings(ui);

  const newTab = ui.page.locator('[data-testid="settings-keybinding-row"][data-keybinding-key="new_tab"]');
  const openSettingsRow = ui.page.locator('[data-testid="settings-keybinding-row"][data-keybinding-key="open_settings"]');

  await expect(newTab.locator('[data-testid="settings-keybinding-display"]')).toContainText(
    expectedShortcutText(config.keybindings.new_tab),
  );
  await expect(openSettingsRow.locator('[data-testid="settings-keybinding-display"]')).toContainText(
    expectedShortcutText(config.keybindings.open_settings),
  );
  await expect(ui.page.locator('[data-testid="settings-keybinding-row"]')).toHaveCount(13);
});
