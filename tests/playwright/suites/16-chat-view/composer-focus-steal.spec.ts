/**
 * Regression: background composer steals focus after async send completes.
 *
 * Root cause: ChatComposer.refocusComposer() had no guard for whether its
 * pane was the active/focused one. The async send() calls refocusComposer()
 * in a finally block — if the user switches to pane B while the send is
 * in-flight, A's finally block fires rAF+setTimeout(0) callbacks that call
 * textarea.focus(), stealing focus from B's composer.
 *
 * Fix: refocusComposer is a no-op when isActive=false (pane not focused).
 * isActive is derived from muxState.focused_pane_id === paneId in
 * PaneSessionChat and stored in a ref so rAF/setTimeout closures see the
 * current value.
 *
 * RED before fix: keyboard text typed into B's composer after switching
 * lands in A's composer instead (focus was stolen).
 * GREEN after fix: focus stays on B; typed text is in B's composer.
 */
import { expect, test } from "../../helpers/fixtures";

test(
  "@golden composer focus is not stolen by background pane after async send",
  async ({ api, ui, paneCleanup }) => {
    // race window so the test can switch to B before A's finally block fires.
    await ui.page.route("**/api/panes/*/send", async (route) => {
      await new Promise((r) => setTimeout(r, 300));
      await route.continue();
    });

    await ui.reconnect();

    const suffix = Date.now();
    const paneA = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
      title: `pw-focus-a-${suffix}`,
    });
    await api.post(`/panes/${paneA.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/panes/${paneA.id}/chat-view`, { enabled: true });

    const paneB = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
      title: `pw-focus-b-${suffix}`,
    });
    await api.post(`/panes/${paneB.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/panes/${paneB.id}/chat-view`, { enabled: true });

    // Focus A and load the UI.
    await api.focusPane(paneA.id);
    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

    // Re-apply the route interception after reload (routes survive reload).
    // Navigate to A's tab.
    if (paneA.tab_id) {
      const tabA = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${paneA.tab_id}"]`)
        .first();
      await expect(tabA).toBeVisible({ timeout: 30_000 });
      await tabA.click();
    }

    const chatA = ui.page.locator(
      `[data-testid="pane-session-chat"][data-pane-id="${paneA.id}"]`,
    );
    await expect(chatA).toBeVisible({ timeout: 30_000 });
    const composerA = chatA.locator('[data-testid="chat-composer"]');
    await expect(composerA).toBeVisible({ timeout: 10_000 });

    // then alive simultaneously, enabling the multi-mount focus-steal scenario.
    if (paneB.tab_id) {
      const tabB = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${paneB.tab_id}"]`)
        .first();
      await expect(tabB).toBeVisible({ timeout: 30_000 });
      await tabB.click();
    }
    const chatB = ui.page.locator(
      `[data-testid="pane-session-chat"][data-pane-id="${paneB.id}"]`,
    );
    await expect(chatB).toBeVisible({ timeout: 30_000 });
    const composerB = chatB.locator('[data-testid="chat-composer"]');
    await expect(composerB).toBeVisible({ timeout: 10_000 });

    // Switch back to A so we can initiate a send from there.
    if (paneA.tab_id) {
      const tabA = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${paneA.tab_id}"]`)
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
    if (paneB.tab_id) {
      const tabB = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${paneB.tab_id}"]`)
        .first();
      await tabB.click();
    }

    // stolen to A, the keystrokes will go to A's composer instead.
    await composerB.click();
    const marker = `pw-b-typed-${suffix}`;
    await ui.page.keyboard.type(marker);

    // Wait well past the 300ms delay + rAF + setTimeout(0) window.
    await ui.page.waitForTimeout(600);

    // Assert: B's composer contains the typed text (focus was NOT stolen).
    await expect(composerB).toHaveValue(marker);

    // Assert: A's composer does NOT contain the marker (it didn't receive the keystrokes).
    const composerAValue = await composerA.inputValue();
    expect(composerAValue).not.toContain(marker);
  },
);
