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
    const plugins = await api.get<Array<{ id: string; capabilities?: { chatBackend?: boolean | { watcher?: boolean } } }>>(
      "/plugins",
    );
    const lm = plugins.find((p) => p.id === PLUGIN_ID);
    const cap = lm?.capabilities?.chatBackend;
    if (cap === true) return false;
    return cap?.watcher === true;
  } catch {
    return false;
  }
}

test("@golden watcher pane is read-only", async ({ api, ui, spawnAgent, paneCleanup }) => {
  const watcherReady = await lmStudioWatcherCapable(api);
  test.skip(!watcherReady, "lmstudio plugin is not watcher-capable in this dev instance");

  await ui.reconnect();

  const orch = await spawnAgent({
    agent: "claude",
    role: "Orchestrator",
    title: `pw-watcher-orch-${Date.now()}`,
  });
  paneCleanup.track(orch.pane_id);

  let watcher: { pane_id: string; tab_id?: string };
  try {
    watcher = await spawnAgent({
      agent: PLUGIN_ID,
      role: "Watcher",
      parent_pane_id: orch.pane_id,
      charter: CHARTER,
      tick_interval_secs: 120,
      title: `pw-watcher-${Date.now()}`,
    });
  } catch (error) {
    test.skip(true, `watcher spawn unavailable: ${String(error).slice(0, 120)}`);
    return;
  }

  await api.focusPane(watcher.pane_id);
  if (watcher.tab_id) {
    const tab = ui.page.locator(`[data-testid="tab-item"][data-tab-id="${watcher.tab_id}"]`).first();
    await expect(tab).toBeVisible({ timeout: 30_000 });
    await tab.click();
  }

  const chat = ui.page.locator(`[data-testid="pane-session-chat"][data-pane-id="${watcher.pane_id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-composer"]')).toHaveCount(0);
  await expect(chat.locator('[data-testid="watcher-readonly-bar"]')).toBeVisible();
  await expect(chat.locator('[data-testid="watcher-charter-preview"]')).toContainText("Observe orchestration health");
  await expect(ui.page.locator('[data-testid="pane-top-bar"] [data-testid="watcher-status-badge"]')).toBeVisible();
});
