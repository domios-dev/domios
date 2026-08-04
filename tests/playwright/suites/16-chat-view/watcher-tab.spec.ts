/**
 * WHAT: watcher tab covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 16-chat-view.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
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

// FIXME(web-mode): /web/ renders only General, so API-workspace tabs never mount.
// Component tests cover this until web-mode can list/switch workspaces.
test.fixme("@golden watcher tab is read-only", async ({ api, ui, spawnAgent, tabCleanup }) => {
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
  tabCleanup.track(orch.tab_id);

  let watcher: { tab_id: string; tab_id?: string };
  try {
    watcher = await spawnAgent({
      agent: PLUGIN_ID,
      role: "Watcher",
      workspace_id: ws.id,
      parent_tab_id: orch.tab_id,
      charter: CHARTER,
      tick_interval_secs: 120,
      title: `pw-watcher-${Date.now()}`,
    });
  } catch (error) {
    test.skip(true, `watcher spawn unavailable: ${String(error).slice(0, 120)}`);
    return;
  }

  await api.post(`/workspaces/${ws.id}/focus`, {});
  await api.focusTab(watcher.tab_id);
  await ui.page.reload();
  await ui.page.waitForLoadState("domcontentloaded");

  const chat = ui.page.locator(`[data-testid="tab-session-chat"][data-tab-id="${watcher.tab_id}"]`);
  await expect(chat).toBeVisible({ timeout: 30_000 });
  await expect(chat.locator('[data-testid="chat-composer"]')).toHaveCount(0);
  await expect(chat.locator('[data-testid="watcher-readonly-bar"]')).toBeVisible();
  await expect(chat.locator('[data-testid="watcher-charter-preview"]')).toContainText("Observe orchestration health");
  await expect(ui.page.locator('[data-testid="tab-top-bar"] [data-testid="watcher-status-badge"]')).toBeVisible();
});
