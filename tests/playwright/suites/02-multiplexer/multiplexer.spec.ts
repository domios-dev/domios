/**
 * WHAT: multiplexer covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 02-multiplexer.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type TabCleanup } from "../../helpers/fixtures";
import { type ApiClient, type Tab } from "../../helpers/api-client";
import { ensureDevConsole, killDevConsole } from "../../helpers/dev-console";

async function createTab(api: ApiClient, tabCleanup: TabCleanup, workspaceId?: string): Promise<string> {
  const created = await api.createTab(workspaceId ? { workspace_id: workspaceId } : {});
  const id = (created as { id?: string }).id;
  expect(id, `create tab response: ${JSON.stringify(created)}`).toBeTruthy();
  tabCleanup.track(id);
  return id!;
}

test("@golden workspace create/focus/close round-trip, UI tracks it", async ({ api, ui }) => {
  const name = `pw-ws-${Date.now()}`;
  await api.createWorkspace(name);

  let workspaces = await api.workspaces();
  const ws = workspaces.find((w) => w.name === name);
  expect(ws, `workspace ${name} missing after create`).toBeDefined();

  // UI renders the new workspace reactively (no manual refresh).
  const wsItem = ui.page.locator(`[data-testid="workspace-item"][data-workspace-name="${name}"]`);
  await expect(wsItem).toBeVisible();

  await api.focusWorkspace(ws!.id);
  workspaces = await api.workspaces();
  expect(workspaces.find((w) => w.id === ws!.id)?.is_active).toBe(true);

  await api.closeWorkspace(ws!.id);
  workspaces = await api.workspaces();
  expect(workspaces.some((w) => w.id === ws!.id)).toBe(false);

  // And it disappears from the UI.
  await expect(wsItem).toHaveCount(0);
});

test("@golden tab create/focus/close round-trip", async ({ api, tabCleanup }) => {
  const before = await api.tabs();
  const tabId = await createTab(api, tabCleanup);

  let tabs = await api.tabs();
  expect(tabs.length).toBe(before.length + 1);
  expect(tabs.some((p) => p.id === tabId)).toBe(true);

  await api.focusTab(tabId);
  tabs = await api.tabs();
  expect(tabs.find((p) => p.id === tabId)?.focused).toBe(true);

  await api.closeTab(tabId);
  tabs = await api.tabs();
  expect(tabs.some((p) => p.id === tabId)).toBe(false);
});

test("@smoke @regression #132: exactly one focused tab across 50 focus switches", async ({ api, tabCleanup }) => {
  const a = await createTab(api, tabCleanup);
  const b = await createTab(api, tabCleanup);
  const targets = [a, b];

  for (let i = 0; i < 50; i++) {
    const target = targets[i % 2];
    await api.focusTab(target);
    const tabs: Tab[] = await api.tabs();
    const focused = tabs.filter((p) => p.focused);
    expect(
      focused.length,
      `iteration ${i}: focused tabs = [${focused.map((p) => p.id).join(", ")}]`,
    ).toBe(1);
    expect(focused[0].id).toBe(target);
  }
});

test("@golden @restart workspace + tab survive a daemon kill/restart", async ({ api, ui, tabCleanup }) => {
  test.setTimeout(300_000);
  const name = `pw-persist-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  expect(ws).toBeDefined();
  const tabId = await createTab(api, tabCleanup, ws!.id);

  // hasn't fired for state this young — snapshot explicitly first.
  await api.post("/session/save");
  killDevConsole();
  await ensureDevConsole();
  // CDP connection died with the old process — reattach.
  await ui.reconnect();

  const workspaces = await api.workspaces();
  const restored = workspaces.find((w) => w.name === name);
  expect(restored, "workspace lost across restart").toBeDefined();
  const tabs = await api.tabs();
  expect(
    tabs.some((p) => p.id === tabId),
    `tab ${tabId} lost across restart`,
  ).toBe(true);

  // UI shows the restored workspace too.
  await expect(
    ui.page.locator(`[data-testid="workspace-item"][data-workspace-name="${name}"]`),
  ).toBeVisible();

  await api.closeWorkspace(restored!.id).catch(() => {});
});
