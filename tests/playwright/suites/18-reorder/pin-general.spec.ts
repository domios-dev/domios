/**
 * WHAT: pin general covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 18-reorder.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page } from "../../helpers/fixtures";
import { ApiClient } from "../../helpers/api-client";

interface StateTab {
  id: string;
  tab_id: string;
  sort_order?: number;
}
interface StateWs {
  id: string;
  tabs: StateTab[];
  sort_order?: number;
  protected?: boolean;
}
interface MuxState {
  workspaces: StateWs[];
  tabs: Record<string, { hidden?: boolean; parent_tab_id?: string | null; protected?: boolean }>;
}

async function muxState(api: ApiClient): Promise<MuxState> {
  return api.get<MuxState>("/mux/state");
}

async function workspaceOrder(api: ApiClient): Promise<string[]> {
  const st = await muxState(api);
  return [...st.workspaces].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0)).map((w) => w.id);
}

/** The General workspace id — the (single) protected workspace. */
async function generalWorkspaceId(api: ApiClient): Promise<string> {
  const st = await muxState(api);
  const ws = st.workspaces.find((w) => w.protected);
  if (!ws) throw new Error("no protected (General) workspace found");
  return ws.id;
}

/** The General tab of a workspace — the tab whose tab is protected. */
async function generalTab(api: ApiClient, wsId: string): Promise<StateTab> {
  const st = await muxState(api);
  const ws = st.workspaces.find((w) => w.id === wsId);
  const tab = ws?.tabs.find((t) => st.tabs[t.tab_id]?.protected);
  if (!tab) throw new Error("no protected (General) tab found");
  return tab;
}

/** Top-level tab ids of a workspace, in sort_order. */
async function topLevelOrder(api: ApiClient, wsId: string): Promise<string[]> {
  const st = await muxState(api);
  const ws = st.workspaces.find((w) => w.id === wsId);
  if (!ws) return [];
  return [...ws.tabs]
    .filter((t) => {
      const tab = st.tabs[t.tab_id];
      return tab && tab.hidden !== true && !tab.parent_tab_id;
    })
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0))
    .map((t) => t.id);
}

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

async function syncUi(page: Page) {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
  // PW_JUSTIFIED_SLEEP: fixed 400ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(400);
}

const tabRow = (wsId: string, tabId: string) =>
  `div[data-dnd-group="tab:${wsId}"]:has(> [data-tab-id="${tabId}"])`;
const wsRow = (wsId: string) =>
  `div[data-dnd-group="workspace:workspaces"]:has(> [data-workspace-id="${wsId}"])`;

async function makeWorkspace(api: ApiClient, name: string): Promise<string> {
  const ws = await api.post<{ id: string }>("/workspaces", { name, activate: true });
  return ws.id;
}
async function makeTab(api: ApiClient, wsId: string): Promise<{ tab_id: string; tab_id: string }> {
  return api.post<{ tab_id: string; tab_id: string }>(`/workspaces/${wsId}/tabs`, {});
}

test("@golden P1 dragging a workspace to the top cannot place it above General", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspaceId(api);
  const a = await makeWorkspace(api, "P1 alpha");
  const b = await makeWorkspace(api, "P1 beta");

  await syncUi(page);
  await page.locator(`[data-workspace-id="${b}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  const before = await workspaceOrder(api);
  expect(before[0], "General starts pinned at the top").toBe(general);

  // Drag beta all the way up — it must not land above General.
  await keyboardDrag(page, wsRow(b), "ArrowUp", before.length + 2);
  // PW_JUSTIFIED_SLEEP: fixed 500ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(500);

  const after = await workspaceOrder(api);
  expect(after[0], "General stays at index 0").toBe(general);
  expect(after.indexOf(b), "beta never rises above General").toBeGreaterThan(0);
  // The above-General drop is rejected outright (snap back) — order unchanged.
  expect(after).toEqual(before);
});

test("@golden P2 dragging a tab to the top cannot place it above the General tab", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspaceId(api);
  const gTab = await generalTab(api, general);
  const t1 = await makeTab(api, general);
  const t2 = await makeTab(api, general);

  await syncUi(page);
  await page.locator(`[data-tab-id="${t2.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  const before = await topLevelOrder(api, general);
  expect(before[0], "General tab starts pinned at the top").toBe(gTab.id);

  // Drag the last tab all the way up — it must not land above the General tab.
  await keyboardDrag(page, tabRow(general, t2.tab_id), "ArrowUp", before.length + 2);
  // PW_JUSTIFIED_SLEEP: fixed 500ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(500);

  const after = await topLevelOrder(api, general);
  expect(after[0], "General tab stays at index 0").toBe(gTab.id);
  expect(after.indexOf(t2.tab_id), "the dragged tab never rises above the General tab").toBeGreaterThan(0);
});

test("@golden P3 the General workspace and General tab are not draggable", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspaceId(api);
  const gTab = await generalTab(api, general);
  // A sibling below General, so there is somewhere to (fail to) move into.
  await makeWorkspace(api, "P3 sibling");

  await syncUi(page);
  await page.locator(wsRow(general)).first().waitFor({ state: "visible", timeout: 20_000 });

  // No drag affordance — the sortable row is disabled.
  await expect(page.locator(wsRow(general)).first()).toHaveAttribute("aria-disabled", "true");
  await expect(page.locator(tabRow(general, gTab.id)).first()).toHaveAttribute("aria-disabled", "true");

  // Attempting to drag General down is a no-op — order is unchanged.
  const wsBefore = await workspaceOrder(api);
  await keyboardDrag(page, wsRow(general), "ArrowDown", 3);
  // PW_JUSTIFIED_SLEEP: fixed 500ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(500);
  expect(await workspaceOrder(api), "General workspace did not move").toEqual(wsBefore);

  const tabsBefore = await topLevelOrder(api, general);
  await keyboardDrag(page, tabRow(general, gTab.id), "ArrowDown", 3);
  // PW_JUSTIFIED_SLEEP: fixed 500ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(500);
  expect(await topLevelOrder(api, general), "General tab did not move").toEqual(tabsBefore);
});
