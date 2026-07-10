/**
 * WHAT: Chat picker default settings honor prompted and explicit config states.
 * WHY: New panes must not surprise the user by silently switching chat UI defaults.
 * ORACLE: Config resets and picker interactions produce the expected persisted default flags.
 */
import { expect, test } from "../../helpers/fixtures";

async function resetChatDefaultConfig(api: import("../../helpers/api-client").ApiClient) {
  await api.request("PATCH", "/config", {
    key: "ui.default_new_agent_pane_chat",
    value: false,
  });
  await api.request("PATCH", "/config", {
    key: "ui.chat_ui_default_prompted",
    value: false,
  });
}

async function selectedModeLabels(page: import("../../helpers/fixtures").Page, paneId: string) {
  const switcher = page
    .locator(`[data-pane-id="${paneId}"] [data-testid="pane-mode-switch"]`)
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
  expect(tabId, "pane did not include tab_id").toBeTruthy();
  const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
  await expect(tab).toBeVisible({ timeout: 30_000 });
  await tab.click();
}

test("@golden accepting chat default clears stale terminal picker overrides across reload", async ({
  api,
  ui,
  paneCleanup,
}) => {
  await resetChatDefaultConfig(api);
  await ui.reconnect();

  const suffix = Date.now();
  const paneA = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-chat-default-a-${suffix}`,
  });
  await api.post(`/panes/${paneA.id}/agent-type`, { agent_type: "claude" });

  const paneB = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
    title: `pw-chat-default-b-${suffix}`,
  });
  await api.post(`/panes/${paneB.id}/agent-type`, { agent_type: "claude" });

  await api.focusPane(paneA.id);
  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openTab(ui.page, paneA.tab_id);

  await ui.page.evaluate((paneId) => {
    localStorage.setItem(`codeterm:pane-ui-mode:${paneId}`, "terminal");
    sessionStorage.setItem(`codeterm:pane-ui-mode:${paneId}`, "terminal");
    localStorage.removeItem("codeterm:pane-ui-mode:terminal-overrides-normalized-for-chat-default");
    localStorage.setItem("codeterm:chat-default-new-agent-pane", "false");
  }, paneB.id);

  const switchA = ui.page
    .locator(`[data-pane-id="${paneA.id}"] [data-testid="pane-mode-switch"]`)
    .first();
  await expect(switchA).toBeVisible({ timeout: 30_000 });
  await switchA.locator('[role="tab"]').filter({ hasText: "Chat" }).click();
  const suggestionA = ui.page
    .locator(`[data-pane-id="${paneA.id}"] [data-testid="chat-default-suggestion"]`)
    .first();
  await expect(suggestionA).toBeVisible({ timeout: 10_000 });
  await suggestionA.locator('[data-testid="chat-default-accept"]').click();
  await expect(ui.page.locator('[data-testid="chat-default-suggestion"]')).toHaveCount(0);
  await expect.poll(async () => {
    const config = await api.get<{ ui?: { default_new_agent_pane_chat?: boolean; chat_ui_default_prompted?: boolean } }>("/config");
    return config.ui?.default_new_agent_pane_chat === true && config.ui?.chat_ui_default_prompted === true;
  }).toBe(true);
  await expect.poll(async () => ui.page.evaluate((paneId) => ({
    chatDefault: localStorage.getItem("codeterm:chat-default-new-agent-pane"),
    paneLocal: localStorage.getItem(`codeterm:pane-ui-mode:${paneId}`),
    paneSession: sessionStorage.getItem(`codeterm:pane-ui-mode:${paneId}`),
  }), paneB.id)).toEqual({
    chatDefault: "true",
    paneLocal: null,
    paneSession: null,
  });

  await openTab(ui.page, paneB.tab_id);

  await expect
    .poll(async () => selectedModeLabels(ui.page, paneB.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);

  await ui.page.reload({ waitUntil: "domcontentloaded" });
  await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openTab(ui.page, paneB.tab_id);

  await expect
    .poll(async () => selectedModeLabels(ui.page, paneB.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);
});
