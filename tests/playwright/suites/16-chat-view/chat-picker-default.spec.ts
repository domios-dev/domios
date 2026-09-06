/**
 * WHAT: A tab's opening view follows chat capability until the user chooses otherwise.
 * WHY: New tabs must not surprise the user by silently switching chat UI defaults.
 * ORACLE: The mode switch selection and the per-tab persisted keys match resolveInitialTabUiMode's branches.
 */
// Assert `resolveInitialTabUiMode` because explicit persisted choice and chat capability are the surviving selection rules.
import { expect, test } from "../../helpers/fixtures";

type ModeLabel = { label: string; selected: boolean };

async function modeSwitch(page: import("../../helpers/fixtures").Page, tabId: string) {
  const switcher = page.locator(`[data-testid="tab-mode-switch"][data-tab-id="${tabId}"]`).first();
  await expect(switcher).toBeVisible({ timeout: 30_000 });
  return switcher;
}

async function selectedModeLabels(
  page: import("../../helpers/fixtures").Page,
  tabId: string,
): Promise<ModeLabel[]> {
  const switcher = await modeSwitch(page, tabId);
  return switcher.locator('[role="tab"]').evaluateAll((tabs) =>
    tabs.map((tab) => ({
      label: (tab.textContent ?? "").trim(),
      selected: tab.getAttribute("aria-selected") === "true",
    })),
  );
}

async function openTab(page: import("../../helpers/fixtures").Page, tabId: string) {
  const tab = page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
  await expect(tab).toBeVisible({ timeout: 30_000 });
  await tab.click();
}

async function reloadTo(page: import("../../helpers/fixtures").Page, tabId: string) {
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });
  await openTab(page, tabId);
}

/** The persisted per-tab choice, read from the keys clientTabUiMode.ts owns. */
async function persistedMode(page: import("../../helpers/fixtures").Page, tabId: string) {
  return page.evaluate((id) => ({
    local: localStorage.getItem(`codeterm:tab-ui-mode:${id}`),
    session: sessionStorage.getItem(`codeterm:tab-ui-mode:${id}`),
  }), tabId);
}

test("@golden a chat-capable tab opens in Chat until an explicit mode choice overrides it across reload", async ({
  api,
  ui,
  tabCleanup,
}) => {
  await ui.reconnect();

  const suffix = Date.now();
  const tabA = await tabCleanup.createTab<{ id: string }>({ title: `pw-chat-default-a-${suffix}` });
  await api.post(`/tabs/${tabA.id}/agent-type`, { agent_type: "claude" });
  const tabB = await tabCleanup.createTab<{ id: string }>({ title: `pw-chat-default-b-${suffix}` });
  await api.post(`/tabs/${tabB.id}/agent-type`, { agent_type: "claude" });

  await api.focusTab(tabA.id);
  await reloadTo(ui.page, tabA.id);

  // A chat-capable agent tab with no persisted choice opens in Chat.
  await expect
    .poll(async () => selectedModeLabels(ui.page, tabA.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);
  expect(await persistedMode(ui.page, tabA.id)).toEqual({ local: null, session: null });

  // An explicit choice is recorded against that tab and only that tab.
  const switchA = await modeSwitch(ui.page, tabA.id);
  await switchA.locator('[role="tab"][data-value="terminal"]').click();
  await expect
    .poll(async () => selectedModeLabels(ui.page, tabA.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: false },
      { label: "Terminal", selected: true },
    ]);
  await expect
    .poll(async () => persistedMode(ui.page, tabA.id), { timeout: 15_000 })
    .toEqual({ local: "terminal", session: "terminal" });
  expect(await persistedMode(ui.page, tabB.id)).toEqual({ local: null, session: null });

  // The explicit choice outranks the capability default across a reload…
  await reloadTo(ui.page, tabA.id);
  await expect
    .poll(async () => selectedModeLabels(ui.page, tabA.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: false },
      { label: "Terminal", selected: true },
    ]);

  // …and the untouched sibling still opens in Chat, so the choice never leaked.
  await openTab(ui.page, tabB.id);
  await expect
    .poll(async () => selectedModeLabels(ui.page, tabB.id), { timeout: 15_000 })
    .toEqual([
      { label: "Chat", selected: true },
      { label: "Terminal", selected: false },
    ]);
});
