/**
 * WHAT: reorder covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 18-reorder.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page } from "../../helpers/fixtures";
import { ApiClient } from "../../helpers/api-client";

interface StateTab {
  id: string;
  pane_id: string;
  sort_order?: number;
}
interface StateWs {
  id: string;
  tabs: StateTab[];
  sort_order?: number;
}
interface MuxState {
  workspaces: StateWs[];
  panes: Record<string, { hidden?: boolean; parent_pane_id?: string | null }>;
}

async function muxState(api: ApiClient): Promise<MuxState> {
  return api.get<MuxState>("/mux/state");
}

/** Tab ids of a workspace's TOP-LEVEL tabs, in sort_order. */
async function topLevelOrder(api: ApiClient, wsId: string): Promise<string[]> {
  const st = await muxState(api);
  const ws = st.workspaces.find((w) => w.id === wsId);
  if (!ws) return [];
  return [...ws.tabs]
    .filter((t) => {
      const pane = st.panes[t.pane_id];
      return pane && pane.hidden !== true && !pane.parent_pane_id;
    })
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0))
    .map((t) => t.id);
}

/** Tab ids that are children of `parentPaneId`, in sort_order. */
async function childOrder(api: ApiClient, wsId: string, parentPaneId: string): Promise<string[]> {
  const st = await muxState(api);
  const ws = st.workspaces.find((w) => w.id === wsId);
  if (!ws) return [];
  return [...ws.tabs]
    .filter((t) => st.panes[t.pane_id]?.parent_pane_id === parentPaneId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0))
    .map((t) => t.id);
}

async function workspaceOrder(api: ApiClient): Promise<string[]> {
  const st = await muxState(api);
  return [...st.workspaces].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0)).map((w) => w.id);
}

function onlyOwned(order: string[], owned: string[]): string[] {
  const ownedSet = new Set(owned);
  return order.filter((id) => ownedSet.has(id));
}

/** Keyboard drag a dnd-kit sortable row: focus, Space (lift), Arrow×n, Space (drop). */
async function keyboardDrag(page: Page, rowSelector: string, key: "ArrowDown" | "ArrowUp", presses: number) {
  const row = page.locator(rowSelector).first();
  await row.waitFor({ state: "visible", timeout: 15_000 });
  await row.focus();
  await page.keyboard.press("Space");
  // PW_JUSTIFIED_SLEEP: fixed 200ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(200);
  for (let i = 0; i < presses; i++) {
    await page.keyboard.press(key);
    // PW_JUSTIFIED_SLEEP: fixed 150ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(150);
  }
  await page.keyboard.press("Space");
  // PW_JUSTIFIED_SLEEP: fixed 400ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(400);
}

/** Force the web client to refetch backend state. Entities created over the
 *  REST API don't always arrive via the live event stream, so we nudge the
 *  store's refresh hook and let SET_STATE re-render the sidebar. */
async function syncUi(page: Page) {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
  // PW_JUSTIFIED_SLEEP: fixed 400ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(400);
}

const tabRow = (wsId: string, tabId: string) =>
  `div[data-dnd-group="tab:${wsId}"]:has(> [data-tab-id="${tabId}"])`;
const childRow = (parentPaneId: string, tabId: string) =>
  `div[data-dnd-group="child:${parentPaneId}"]:has(> [data-tab-id="${tabId}"])`;
const wsRow = (wsId: string) =>
  `div[data-dnd-group="workspace:workspaces"]:has(> [data-workspace-id="${wsId}"])`;

async function makeWorkspace(api: ApiClient, name: string): Promise<string> {
  const ws = await api.post<{ id: string }>("/workspaces", { name, activate: true });
  return ws.id;
}
async function makeTab(api: ApiClient, wsId: string): Promise<{ tab_id: string; pane_id: string }> {
  return api.post<{ tab_id: string; pane_id: string }>(`/workspaces/${wsId}/tabs`, {});
}

test("@golden C1 a top-level tab reorders within its workspace", async ({ api, ui }) => {
  const { page } = ui;
  const ws = await makeWorkspace(api, "C1 reorder");
  const t1 = await makeTab(api, ws);
  const t2 = await makeTab(api, ws);
  const t3 = await makeTab(api, ws);

  await syncUi(page);
  await page.locator(`[data-tab-id="${t3.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  const owned = [t1.tab_id, t2.tab_id, t3.tab_id];
  expect(onlyOwned(await topLevelOrder(api, ws), owned)).toEqual(owned);

  // Drag the first tab down two slots → it lands last.
  await keyboardDrag(page, tabRow(ws, t1.tab_id), "ArrowDown", 2);

  await expect
    .poll(async () => onlyOwned(await topLevelOrder(api, ws), owned).join(","), { timeout: 15_000 })
    .toBe([t2.tab_id, t3.tab_id, t1.tab_id].join(","));
});

test("@golden C2 a child reorders within its parent group and cannot leave it", async ({ api, ui }) => {
  const { page } = ui;
  const ws = await makeWorkspace(api, "C2 hierarchy");
  const parent = await makeTab(api, ws);
  const c1 = await makeTab(api, ws);
  const c2 = await makeTab(api, ws);
  await api.post(`/panes/${c1.pane_id}/role`, { role: "Worker", parent_pane_id: parent.pane_id });
  await api.post(`/panes/${c2.pane_id}/role`, { role: "Worker", parent_pane_id: parent.pane_id });

  await syncUi(page);
  // Children render nested under the parent in their own drop group.
  await page.locator(childRow(parent.pane_id, c2.tab_id)).first().waitFor({ state: "visible", timeout: 20_000 });
  const ownedChildren = [c1.tab_id, c2.tab_id];
  expect(onlyOwned(await childOrder(api, ws, parent.pane_id), ownedChildren)).toEqual(ownedChildren);

  // Positive: reorder the two siblings → order flips, both stay children.
  await keyboardDrag(page, childRow(parent.pane_id, c1.tab_id), "ArrowDown", 1);
  await expect
    .poll(async () => onlyOwned(await childOrder(api, ws, parent.pane_id), ownedChildren).join(","), { timeout: 15_000 })
    .toBe([c2.tab_id, c1.tab_id].join(","));

  // is rejected — the child must remain a child of the same parent.
  const topBefore = await topLevelOrder(api, ws);
  await keyboardDrag(page, childRow(parent.pane_id, c2.tab_id), "ArrowUp", 4);
  // PW_JUSTIFIED_SLEEP: fixed 500ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(500);
  const st = await muxState(api);
  expect(st.panes[c2.pane_id]?.parent_pane_id, "child must not escape its parent group").toBe(parent.pane_id);
  expect(await topLevelOrder(api, ws), "top-level group must not absorb the child").toEqual(topBefore);
});

test("@golden C3 workspaces reorder among themselves", async ({ api, ui }) => {
  const { page } = ui;
  const a = await makeWorkspace(api, "C3 alpha");
  const b = await makeWorkspace(api, "C3 beta");

  await syncUi(page);
  await page.locator(`[data-workspace-id="${b}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  const before = await workspaceOrder(api);
  const ia = before.indexOf(a);
  const ib = before.indexOf(b);
  expect(ia).toBeGreaterThanOrEqual(0);
  expect(ib).toBeGreaterThan(ia);

  // Drag beta up one → beta now precedes alpha.
  await keyboardDrag(page, wsRow(b), "ArrowUp", 1);
  await expect
    .poll(
      async () => {
        const order = await workspaceOrder(api);
        return order.indexOf(b) < order.indexOf(a);
      },
      { timeout: 15_000 },
    )
    .toBe(true);
});

test("@golden C4 reordered order survives a reload", async ({ api, ui }) => {
  const { page } = ui;
  const ws = await makeWorkspace(api, "C4 persist");
  const t1 = await makeTab(api, ws);
  const t2 = await makeTab(api, ws);
  const t3 = await makeTab(api, ws);

  await syncUi(page);
  await page.locator(`[data-tab-id="${t3.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  await keyboardDrag(page, tabRow(ws, t3.tab_id), "ArrowUp", 2);
  await expect
    .poll(async () => (await topLevelOrder(api, ws)).join(","), { timeout: 15_000 })
    .toBe([t3.tab_id, t1.tab_id, t2.tab_id].join(","));

  // Persist to disk, then reload the client and confirm the sidebar re-sorts.
  await api.post("/session/save");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(`[data-tab-id="${t1.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  const domOrder = await page.evaluate((wsId: string) =>
    Array.from(document.querySelectorAll(`div[data-dnd-group="tab:${wsId}"]`))
      .map((row) => row.querySelector("[data-tab-id]")?.getAttribute("data-tab-id"))
      .filter(Boolean),
  ws);
  // Rendered top-level rows reflect the persisted order.
  expect(domOrder).toEqual([t3.tab_id, t1.tab_id, t2.tab_id]);
  expect(await topLevelOrder(api, ws)).toEqual([t3.tab_id, t1.tab_id, t2.tab_id]);
});
