/**
 * WHAT: composer focus steal covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 16-chat-view.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";

test(
  "@golden composer focus is not stolen by background tab after async send",
  async ({ api, ui, tabCleanup }) => {
    // race window so the test can switch to B before A's finally block fires.
    await ui.page.route("**/api/tabs/*/send", async (route) => {
      await new Promise((r) => setTimeout(r, 300));
      await route.continue();
    });

    await ui.reconnect();

    const suffix = Date.now();
    const tabA = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
      title: `pw-focus-a-${suffix}`,
    });
    await api.post(`/tabs/${tabA.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/tabs/${tabA.id}/chat-view`, { enabled: true });

    const tabB = await tabCleanup.createTab<{ id: string; tab_id?: string }>({
      title: `pw-focus-b-${suffix}`,
    });
    await api.post(`/tabs/${tabB.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/tabs/${tabB.id}/chat-view`, { enabled: true });

    // Focus A and load the UI.
    await api.focusTab(tabA.id);
    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

    // Re-apply the route interception after reload (routes survive reload).
    // Navigate to A's tab.
    const rowA = ui.page
      .locator(`[data-testid="tab-item"][data-tab-id="${tabA.id}"]`)
      .first();
    await expect(rowA).toBeVisible({ timeout: 30_000 });
    await rowA.click();

    const chatA = ui.page.locator(
      `[data-testid="tab-session-chat"][data-tab-id="${tabA.id}"]`,
    );
    await expect(chatA).toBeVisible({ timeout: 30_000 });
    const composerA = chatA.locator('[data-testid="chat-composer"]');
    await expect(composerA).toBeVisible({ timeout: 10_000 });

    // then alive simultaneously, enabling the multi-mount focus-steal scenario.
    if (tabB.tab_id) {
      const tabB = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${tabB.tab_id}"]`)
        .first();
      await expect(tabB).toBeVisible({ timeout: 30_000 });
      await tabB.click();
    }
    const chatB = ui.page.locator(
      `[data-testid="tab-session-chat"][data-tab-id="${tabB.id}"]`,
    );
    await expect(chatB).toBeVisible({ timeout: 30_000 });
    const composerB = chatB.locator('[data-testid="chat-composer"]');
    await expect(composerB).toBeVisible({ timeout: 10_000 });

    // Switch back to A so we can initiate a send from there.
    if (tabA.tab_id) {
      const tabA = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${tabA.tab_id}"]`)
        .first();
      await tabA.click();
    }
    await expect(chatA).toBeVisible({ timeout: 10_000 });

    // Fill A's composer and press Enter — this triggers the async send.
    // The route intercept delays it by 300ms, giving us time to switch to B.
    await composerA.click();
    await composerA.fill("hello from A");
    await composerA.press("Enter");

    // Switch to B immediately (within the 300ms send delay window).
    if (tabB.tab_id) {
      const tabB = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${tabB.tab_id}"]`)
        .first();
      await tabB.click();
    }

    // stolen to A, the keystrokes will go to A's composer instead.
    await composerB.click();
    const marker = `pw-b-typed-${suffix}`;
    await ui.page.keyboard.type(marker);

    // Wait well past the 300ms delay + rAF + setTimeout(0) window.
    // PW_JUSTIFIED_SLEEP: fixed 600ms sampling/settle window; no app event exists for this probe.
    await ui.page.waitForTimeout(600);

    // Assert: B's composer contains the typed text (focus was NOT stolen).
    await expect(composerB).toHaveValue(marker);

    // Assert: A's composer does NOT contain the marker (it didn't receive the keystrokes).
    const composerAValue = await composerA.inputValue();
    expect(composerAValue).not.toContain(marker);
  },
);
