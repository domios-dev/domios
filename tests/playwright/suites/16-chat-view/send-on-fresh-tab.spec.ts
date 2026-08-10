/**
 * WHAT: send on fresh tab covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 16-chat-view.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";

test(
  "@golden ChatView send on a fresh tab never shows 'Tab has no PTY' error",
  async ({ api, ui, tabCleanup }) => {
    await ui.reconnect();

    // Create a bare shell tab — pty_id starts null (eager defaults to false).
    const tab = await tabCleanup.createTab<{ id: string }>({
      title: `pw-fresh-tab-send-${Date.now()}`,
    });

    // event; the tab still has no pty_id at this point.
    await api.post(`/tabs/${tab.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/tabs/${tab.id}/chat-view`, { enabled: true });
    await api.focusTab(tab.id);

    // POST /tabs answers with ids only, so read the title off the tab record
    // that owns it — the chat region is named from exactly that value.
    const created = (await api.tabs()).find((candidate) => candidate.id === tab.id);
    expect(created, "the created tab is listed by the daemon").toBeTruthy();
    const chatRegionName = `Tab chat: ${created!.title ?? tab.id}`;

    // Reload so the tab renders fresh — pty_id is null on the first paint;
    // xterm mount + PTY link happen asynchronously afterward.
    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

    // Click into the tab.
    const row = ui.page
      .locator(`[data-testid="tab-item"][data-tab-id="${tab.id}"]`)
      .first();
    await expect(row).toBeVisible({ timeout: 30_000 });
    await row.click();

    const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tab.id}"]`);
    await expect(chat).toBeVisible({ timeout: 30_000 });
    await expect(chat).toHaveRole("region");
    await expect(chat, "the chat region is named from the tab's own title")
      .toHaveAttribute("aria-label", chatRegionName);

    const conversation = chat.getByRole("log", { name: "Conversation" });
    const composer = chat.getByRole("textbox", { name: "Message composer" });
    await expect(conversation).toBeVisible({ timeout: 10_000 });
    await expect(composer).toBeVisible({ timeout: 10_000 });

    // still be null, which would have triggered "Tab has no PTY" before the fix.
    const marker = `pw-fresh-no-pty-send-${Date.now()}`;
    await composer.fill(marker);
    await composer.press("Enter");

    // The rendered user message is the send completing — wait on it, not on a
    // clock, then assert the failure toast never accompanied it.
    const userMsg = conversation
      .getByRole("article", { name: "user message" })
      .filter({ hasText: marker });
    await expect(userMsg).toBeVisible({ timeout: 30_000 });

    const tabHasNoPtyToast = ui.page
      .locator('[data-testid="notification-toast"]')
      .filter({ hasText: "Tab has no PTY" });
    await expect(tabHasNoPtyToast).toHaveCount(0);
  },
);
