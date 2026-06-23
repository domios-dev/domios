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

async function setChatPaneDefault(
  api: { request<T>(method: string, apiPath: string, body?: unknown): Promise<T> },
  original: AppConfig,
  enabled: boolean,
): Promise<void> {
  await api.request<AppConfig>("PATCH", "/config", {
    ...original,
    ui: {
      ...original.ui,
      default_new_agent_pane_chat: enabled,
      chat_ui_default_prompted: true,
    },
  });
}

async function resetChatDefaultBrowserMirror(ui: ManagedUiSession, enabled: boolean): Promise<void> {
  await ui.page.evaluate((value) => {
    localStorage.setItem("codeterm:chat-default-new-agent-pane", value ? "true" : "false");
    localStorage.removeItem("codeterm:pane-ui-mode:terminal-overrides-normalized-for-chat-default");
  }, enabled);
}

async function expectPaneModeSwitch(
  ui: ManagedUiSession,
  paneId: string,
  expected: { order: string[]; selected: string },
): Promise<void> {
  const switcher = ui.page.locator(`[data-pane-id="${paneId}"] [data-testid="pane-mode-switch"]`);
  await expect(switcher).toBeVisible({ timeout: 30_000 });
  const tabs = switcher.getByRole("tab");
  await expect.poll(async () => {
    return tabs.evaluateAll((nodes) => nodes.map((node) => node.textContent?.trim() ?? ""));
  }).toEqual(expected.order);
  await expect(switcher.getByRole("tab", { name: expected.selected })).toHaveAttribute("aria-selected", "true");
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
  await expect(ui.page.locator('[data-testid="settings-keybinding-row"]')).toHaveCount(14);
});

test("@golden General Settings chat default updates picker order and survives reload", async ({
  api,
  ui,
  paneCleanup,
}) => {
  const original = await api.get<AppConfig>("/config");
  let paneId: string | undefined;

  try {
    await setChatPaneDefault(api, original, false);
    await ui.reconnect();
    await resetChatDefaultBrowserMirror(ui, false);

    const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
      title: `pw-chat-default-picker-${Date.now()}`,
    });
    paneId = pane.id;
    await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
    await api.focusPane(pane.id);

    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
    if (pane.tab_id) {
      const tab = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).first();
      await expect(tab).toBeVisible({ timeout: 30_000 });
      await tab.click();
    }
    await expectPaneModeSwitch(ui, pane.id, { order: ["Terminal", "Chat"], selected: "Terminal" });

    await ui.page.evaluate((id) => {
      localStorage.setItem(`codeterm:pane-ui-mode:${id}`, "terminal");
      sessionStorage.setItem(`codeterm:pane-ui-mode:${id}`, "terminal");
    }, pane.id);

    await openSettings(ui);
    const chatDefaultToggle = ui.page.locator(
      '[data-settings-toggle="use-chat-as-the-default-view-for-new-agent-tabs"][role="switch"]',
    );
    await expect(chatDefaultToggle).toHaveAttribute("aria-checked", "false");
    await chatDefaultToggle.click();
    await expect(chatDefaultToggle).toHaveAttribute("aria-checked", "true");
    await expect.poll(async () => {
      const cfg = await api.get<AppConfig>("/config");
      return cfg.ui?.default_new_agent_pane_chat === true && cfg.ui?.chat_ui_default_prompted === true;
    }).toBe(true);

    await ui.page.locator('[data-testid="settings-close"]').click();
    await expectPaneModeSwitch(ui, pane.id, { order: ["Chat", "Terminal"], selected: "Chat" });

    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
    if (pane.tab_id) {
      const tab = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`).first();
      await expect(tab).toBeVisible({ timeout: 30_000 });
      await tab.click();
    }
    await expectPaneModeSwitch(ui, pane.id, { order: ["Chat", "Terminal"], selected: "Chat" });
  } finally {
    await restoreConfig(api, original);
    if (paneId) {
      await ui.page.evaluate((id) => {
        localStorage.removeItem(`codeterm:pane-ui-mode:${id}`);
        sessionStorage.removeItem(`codeterm:pane-ui-mode:${id}`);
        localStorage.removeItem("codeterm:pane-ui-mode:terminal-overrides-normalized-for-chat-default");
      }, paneId).catch(() => {});
    }
  }
});
