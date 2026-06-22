/**
 * Regression: "Chat error: Pane has no PTY" on a fresh pane.
 *
 * Root cause: ChatComposer.tsx threw "Pane has no PTY" before calling
 * chat_send_text when currentPtyId was null — which happens on freshly
 * created panes before the backend links the PTY.  The backend handler
 * (send_text_when_ready → await_pty_link_blocking) already waits up to
 * 15s for the PTY, so the frontend guard was wrong.
 *
 * Fix: move the currentPtyId guard inside the fallbackCommand branch only
 * (CLI fallback commands write_pty_string directly and DO need the id).
 * Regular sends go through chat_send_text unconditionally.
 *
 * RED before fix: sending immediately after a fresh chat-view pane renders
 * produces a "Chat error: Pane has no PTY" notification toast.
 * GREEN after fix: no such toast; the backend waits for the PTY internally.
 */
import { expect, test } from "../../helpers/fixtures";

test(
  "@golden ChatView send on a fresh pane never shows 'Pane has no PTY' error",
  async ({ api, ui, paneCleanup }) => {
    await ui.reconnect();

    // Create a bare shell pane — pty_id starts null (eager defaults to false).
    const pane = await paneCleanup.createPane<{ id: string; tab_id?: string }>({
      title: `pw-fresh-pane-send-${Date.now()}`,
    });

    // Set agent_type + enable chat-view.  Each call emits a mux state-changed
    // event; the pane still has no pty_id at this point.
    await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
    await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
    await api.focusPane(pane.id);

    // Reload so the pane renders fresh — pty_id is null on the first paint;
    // xterm mount + PTY link happen asynchronously afterward.
    await ui.page.reload({ waitUntil: "domcontentloaded" });
    await ui.page.waitForSelector("#root > *", { state: "attached", timeout: 90_000 });

    // Click into the tab.
    if (pane.tab_id) {
      const tab = ui.page
        .locator(`[data-testid="tab-item"][data-tab-id="${pane.tab_id}"]`)
        .first();
      await expect(tab).toBeVisible({ timeout: 30_000 });
      await tab.click();
    }

    // Wait for the chat view to render.  The comment in TerminalPane says
    // "No session-readiness gate" — PaneSessionChat renders immediately,
    // including when pty_id is still null.
    const chat = ui.page.locator(
      `[data-testid="pane-session-chat"][data-pane-id="${pane.id}"]`,
    );
    await expect(chat).toBeVisible({ timeout: 30_000 });

    const composer = chat.locator('[data-testid="chat-composer"]');
    await expect(composer).toBeVisible({ timeout: 10_000 });

    // Send a message immediately — this is the race window where pty_id may
    // still be null, which would have triggered "Pane has no PTY" before the fix.
    const marker = `pw-fresh-no-pty-send-${Date.now()}`;
    await composer.fill(marker);
    await composer.press("Enter");

    // Give any error notification time to appear.
    await ui.page.waitForTimeout(1_500);

    // Assert: no "Pane has no PTY" toast.
    const paneHasNoPtyToast = ui.page
      .locator('[data-testid="notification-toast"]')
      .filter({ hasText: "Pane has no PTY" });
    await expect(paneHasNoPtyToast).toHaveCount(0);

    // Assert: the optimistic user message was added to the chat history
    // (addOptimistic fires before the backend call, so this confirms the send
    // path reached the backend rather than short-circuiting on the error).
    const userMsg = chat
      .locator('[data-testid="chat-message"][data-message-role="user"]')
      .filter({ hasText: marker });
    await expect(userMsg).toBeVisible({ timeout: 10_000 });
  },
);
