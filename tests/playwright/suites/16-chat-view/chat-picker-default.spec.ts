/**
 * WHAT: Chat picker default settings honor prompted and explicit config states.
 * WHY: New tabs must not surprise the user by silently switching chat UI defaults.
 * ORACLE: Config resets and picker interactions produce the expected persisted default flags.
 */
import { expect, test } from "../../helpers/fixtures";

async function resetChatDefaultConfig(api: import("../../helpers/api-client").ApiClient) {
  await api.request("PATCH", "/config", {
    key: "ui.default_new_agent_tab_chat",
    value: false,
  });
  await api.request("PATCH", "/config", {
    key: "ui.chat_ui_default_prompted",
    value: false,
  });
}

async function selectedModeLabels(page: import("../../helpers/fixtures").Page, tabId: string) {
  const switcher = page
    .locator(`[data-tab-id="${tabId}"] [data-testid="tab-mode-switch"]`)
    .first();
  await expect(switcher).toBeVisible({ timeout: 30_000 });
  return switcher.locator('[role="tab"]').evaluateAll((tabs) =>
    tabs.map((tab) => ({
      label: (tab.textContent ?? "").trim(),
      selected: tab.getAttribute("aria-selected") === "true",
    })),
  );
}

async function openTab(page: import("../../helpers/fixtures").Page, tabId: string | undefined) {
  expect(tabId, "tab did not include tab_id").toBeTruthy();
  const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
  await expect(tab).toBeVisible({ timeout: 30_000 });
  await tab.click();
}

test("@golden accepting chat default clears stale terminal picker overrides across reload", async ({
  api,
  ui,
  tabCleanup,
}) => {
  await resetChatDefaultConfig(api);
  await ui.reconnect();

  const suffix = Date.now();
  const tabA = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
    title: `pw-chat-default-a-${suffix}`,
  });
  await api.post(`/tabs/${tabA.id}/agent-type`, { agent_type: "claude" });

  const tabB = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
    title: `pw-chat-default-b-${suffix}`,
  });
  await api.post(`/tabs/${tabB.id}/agent-type`, { agent_type: "claude" });

  await api.focusTab(tabA.id);
  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openTab(ui.page, tabA.tab_id);

  await ui.page.evaluate((tabId) => {
    localStorage.setItem(`codeterm:tab-ui-mode:${tabId}`, "terminal");
    sessionStorage.setItem(`codeterm:tab-ui-mode:${tabId}`, "terminal");
    localStorage.removeItem("codeterm:tab-ui-mode:terminal-overrides-normalized-for-chat-default");
    localStorage.setItem("codeterm:chat-default-new-agent-tab", "false");
  }, tabB.id);

  const switchA = ui.page
    .locator(`[data-tab-id="${tabA.id}"] [data-testid="tab-mode-switch"]`)
    .first();
  await expect(switchA).toBeVisible({ timeout: 30_000 });
  await switchA.locator('[role="tab"]').filter({ hasText: "Chat" }).click();
  const suggestionA = ui.page
    .locator(`[data-tab-id="${tabA.id}"] [data-testid="chat-default-suggestion"]`)
    .first();
  await expect(suggestionA).toBeVisible({ timeout: 10_000 });
  await suggestionA.locator('[data-testid="chat-default-accept"]').click();
  await expect(ui.page.locator('[data-testid="chat-default-suggestion"]')).toHaveCount(0);
  await expect.poll(async () => {
    const config = await api.get<{ ui?: { default_new_agent_tab_chat?: boolean; chat_ui_default_prompted?: boolean } }>("/config");
    return config.ui?.default_new_agent_tab_chat === true && config.ui?.chat_ui_default_prompted === true;
  }).toBe(true);
  await expect.poll(async () => ui.page.evaluate((tabId) => ({
    chatDefault: localStorage.getItem("codeterm:chat-default-new-agent-tab"),
    tabLocal: localStorage.getItem(`codeterm:tab-ui-mode:${tabId}`),
    tabSession: sessionStorage.getItem(`codeterm:tab-ui-mode:${tabId}`),
  }), tabB.id)).toEqual({
    chatDefault: "true",
    tabLocal: null,
    tabSession: null,
  });

  await openTab(ui.page, tabB.tab_id);

  await expect
    .poll(async () => selectedModeLabels(ui.page, tabB.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);

  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openTab(ui.page, tabB.tab_id);

  await expect
    .poll(async () => selectedModeLabels(ui.page, tabB.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);
});
