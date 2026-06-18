/**
 * 02-multiplexer — workspace/pane lifecycle against the live dev console,
 * verified through BOTH the API and the rendered UI (CDP).
 *
 * Includes the #132 lesson as e2e: across 50 focus switches the projected
 * pane list must mark exactly one focused pane every single time.
 */
import { expect, test, type PaneCleanup } from "../../helpers/fixtures";
import { type ApiClient, type Pane } from "../../helpers/api-client";
import { ensureDevConsole, killDevConsole } from "../../helpers/dev-console";

async function createPane(api: ApiClient, paneCleanup: PaneCleanup, workspaceId?: string): Promise<string> {
  const created = await api.createPane(workspaceId ? { workspace_id: workspaceId } : {});
  const id = (created as { id?: string }).id;
  expect(id, `create pane response: ${JSON.stringify(created)}`).toBeTruthy();
  paneCleanup.track(id);
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

test("@golden pane create/focus/close round-trip", async ({ api, paneCleanup }) => {
  const before = await api.panes();
  const paneId = await createPane(api, paneCleanup);

  let panes = await api.panes();
  expect(panes.length).toBe(before.length + 1);
  expect(panes.some((p) => p.id === paneId)).toBe(true);

  await api.focusPane(paneId);
  panes = await api.panes();
  expect(panes.find((p) => p.id === paneId)?.focused).toBe(true);

  await api.closePane(paneId);
  panes = await api.panes();
  expect(panes.some((p) => p.id === paneId)).toBe(false);
});

test("@smoke @regression #132: exactly one focused pane across 50 focus switches", async ({ api, paneCleanup }) => {
  const a = await createPane(api, paneCleanup);
  const b = await createPane(api, paneCleanup);
  const targets = [a, b];

  for (let i = 0; i < 50; i++) {
    const target = targets[i % 2];
    await api.focusPane(target);
    const panes: Pane[] = await api.panes();
    const focused = panes.filter((p) => p.focused);
    expect(
      focused.length,
      `iteration ${i}: focused panes = [${focused.map((p) => p.id).join(", ")}]`,
    ).toBe(1);
    expect(focused[0].id).toBe(target);
  }
});

test("@golden workspace + pane survive a daemon kill/restart", async ({ api, ui, paneCleanup }) => {
  test.setTimeout(300_000);
  const name = `pw-persist-${Date.now()}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  expect(ws).toBeDefined();
  const paneId = await createPane(api, paneCleanup, ws!.id);

  // Force-kill bypasses the close-to-tray save and the 30s auto-save timer
  // hasn't fired for state this young — snapshot explicitly first.
  await api.post("/session/save");
  killDevConsole();
  await ensureDevConsole();
  // CDP connection died with the old process — reattach.
  await ui.reconnect();

  const workspaces = await api.workspaces();
  const restored = workspaces.find((w) => w.name === name);
  expect(restored, "workspace lost across restart").toBeDefined();
  const panes = await api.panes();
  expect(
    panes.some((p) => p.id === paneId),
    `pane ${paneId} lost across restart`,
  ).toBe(true);

  // UI shows the restored workspace too.
  await expect(
    ui.page.locator(`[data-testid="workspace-item"][data-workspace-name="${name}"]`),
  ).toBeVisible();

  await api.closeWorkspace(restored!.id).catch(() => {});
});
