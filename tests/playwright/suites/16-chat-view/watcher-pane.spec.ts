/**
 * Watcher pane read-only composer — chatBackend machine+watcher UI contract.
 *
 * Run (orchestrator / QA only, against healthy dev):
 *   curl -s http://127.0.0.1:7686/api/health
 *   npx playwright test tests/playwright/suites/16-chat-view/watcher-pane.spec.ts \
 *     -g "watcher pane is read-only" --timeout=30000 --reporter=line --global-timeout=600000
 */
import { expect, test } from "../../helpers/fixtures";

const PLUGIN_ID = "lmstudio";
const CHARTER = "Observe orchestration health. Report stalled workers.";

async function lmStudioWatcherCapable(api: import("../../helpers/api-client").ApiClient): Promise<boolean> {
  try {
    const providers = await api.get<Array<{ id: string; supports_watcher?: boolean }>>("/agents/providers");
    return providers.find((p) => p.id === PLUGIN_ID)?.supports_watcher === true;
  } catch {
    return false;
  }
}

// FIXME(web-mode): the /web/ client renders only the General workspace — panes
// spawned into API-created workspaces never appear, so this spec cannot mount
// the watcher pane UI. Unblock by teaching web-mode to list/switch workspaces
// (or by pinning the fixture into the General workspace). UI contract is
// covered by component tests (PaneSessionChat.watcher, MessageBubble.watcher).
test.fixme("@golden watcher pane is read-only", async ({ api, ui, spawnAgent, paneCleanup }) => {
  const watcherReady = await lmStudioWatcherCapable(api);
  test.skip(!watcherReady, "lmstudio plugin is not watcher-capable in this dev instance");

  await ui.reconnect();

  const ws = await api.post<{ id: string }>("/workspaces", {
    name: `pw-watcher-ws-${Date.now()}`,
    cwd: "/tmp",
  });

  const orch = await spawnAgent({
    agent: "claude",
    role: "Orchestrator",
    workspace_id: ws.id,
    title: `pw-watcher-orch-${Date.now()}`,
  });
  paneCleanup.track(orch.pane_id);

  let watcher: { pane_id: string; tab_id?: string };
  try {
    watcher = await spawnAgent({
      agent: PLUGIN_ID,
      role: "Watcher",
      workspace_id: ws.id,
      parent_pane_id: orch.pane_id,
      charter: CHARTER,
      tick_interval_secs: 120,
      title: `pw-watcher-${Date.now()}`,
    });
  } catch (error) {
    test.skip(true, `watcher spawn unavailable: ${String(error).slice(0, 120)}`);
    return;
  }

  await api.post(`/workspaces/${ws.id}/focus`, {});
  await api.focusPane(watcher.pane_id);
  await ui.page.reload();
  await ui.page.waitForLoadState("domcontentloaded");

  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${watcher.pane_id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-composer"]')).toHaveCount(0);
  await expect(chat.locator('[data-testid="watcher-readonly-bar"]')).toBeVisible();
  await expect(chat.locator('[data-testid="watcher-charter-preview"]')).toContainText("Observe orchestration health");
  await expect(ui.page.locator('[data-testid="pane-top-bar"] [data-testid="watcher-status-badge"]')).toBeVisible();
});
