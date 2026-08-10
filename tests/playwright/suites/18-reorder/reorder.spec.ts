/**
 * WHAT: reorder covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 18-reorder.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page } from "../../helpers/fixtures";
import { ApiClient } from "../../helpers/api-client";

/** `/mux/state` shape. `Workspace.tabs` is a list of tab ids
 *  (src-tauri/crates/codeterm-pty/src/multiplexer/types.rs), never objects. */
interface StateTab {
  id: string;
  sort_order?: number;
  hidden?: boolean;
  parent_tab_id?: string | null;
  protected?: boolean;
}
interface StateWs {
  id: string;
  tabs: string[];
  sort_order?: number;
  protected?: boolean;
}
interface MuxState {
  workspaces: StateWs[];
  tabs: Record<string, StateTab>;
}

async function muxState(api: ApiClient): Promise<MuxState> {
  return api.get<MuxState>("/mux/state");
}

const bySortOrder = (a: { sort_order?: number }, b: { sort_order?: number }) =>
  (a.sort_order ?? 0) - (b.sort_order ?? 0);

/** Resolve a workspace's tab ids to their tab records, dropping unknown ids. */
function workspaceTabs(state: MuxState, wsId: string): StateTab[] {
  const ws = state.workspaces.find((w) => w.id === wsId);
  if (!ws) return [];
  return ws.tabs.map((tabId) => state.tabs[tabId]).filter((tab): tab is StateTab => Boolean(tab));
}

/** Tab ids of a workspace's TOP-LEVEL tabs, in sort_order. */
async function topLevelOrder(api: ApiClient, wsId: string): Promise<string[]> {
  const state = await muxState(api);
  return workspaceTabs(state, wsId)
    .filter((tab) => tab.hidden !== true && !tab.parent_tab_id)
    .sort(bySortOrder)
    .map((tab) => tab.id);
}

/** Tab ids that are children of `parentTabId`, in sort_order. */
async function childOrder(api: ApiClient, wsId: string, parentTabId: string): Promise<string[]> {
  const state = await muxState(api);
  return workspaceTabs(state, wsId)
    .filter((tab) => tab.parent_tab_id === parentTabId)
    .sort(bySortOrder)
    .map((tab) => tab.id);
}

/** Ordinary (non-protected) workspace ids in sort_order — the set the sidebar
 *  renders as one sortable group; protected General is partitioned out of it. */
async function ordinaryWorkspaceOrder(api: ApiClient): Promise<string[]> {
  const state = await muxState(api);
  return state.workspaces.filter((ws) => !ws.protected).sort(bySortOrder).map((ws) => ws.id);
}

function onlyOwned(order: string[], owned: string[]): string[] {
  const ownedSet = new Set(owned);
  return order.filter((id) => ownedSet.has(id));
}

type ArrowKey = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

/** Keyboard drag a dnd-kit sortable row: focus, Space (lift), Arrow×n, Space
 *  (drop). SortableRow stamps `data-dnd-dragging` for the lifted row, so both
 *  ends of the gesture wait on an observable DOM signal instead of a delay. */
async function keyboardDrag(page: Page, rowSelector: string, key: ArrowKey, presses: number) {
  const row = page.locator(rowSelector).first();
  await row.waitFor({ state: "visible", timeout: 15_000 });
  await row.focus();
  await page.keyboard.press("Space");
  await expect(row).toHaveAttribute("data-dnd-dragging", "true", { timeout: 15_000 });
  for (let i = 0; i < presses; i++) {
    await page.keyboard.press(key);
  }
  await page.keyboard.press("Space");
  await expect(row).not.toHaveAttribute("data-dnd-dragging", "true", { timeout: 15_000 });
}

/** Workspaces render as a CARD GRID (rectSortingStrategy), so which card a
 *  single arrow reaches depends on where the cards actually landed, not on
 *  their index. Find the subject's rendered predecessor and the arrow that
 *  walks onto it. */
async function precedingCard(
  page: Page,
  subjectId: string,
  candidateIds: string[],
): Promise<{ id: string; key: ArrowKey }> {
  const boxes = new Map<string, { x: number; y: number; width: number; height: number }>();
  for (const id of [subjectId, ...candidateIds]) {
    const box = await page.locator(wsRow(id)).first().boundingBox();
    if (box) boxes.set(id, box);
  }
  const subject = boxes.get(subjectId);
  if (!subject) throw new Error(`workspace card ${subjectId} is not laid out`);
  let sameRow: { id: string; x: number } | null = null;
  let above: { id: string; y: number } | null = null;
  for (const [id, box] of boxes) {
    if (id === subjectId) continue;
    if (Math.abs(box.y - subject.y) <= subject.height / 2) {
      if (box.x < subject.x && (!sameRow || box.x > sameRow.x)) sameRow = { id, x: box.x };
    } else if (box.y < subject.y && Math.abs(box.x - subject.x) <= subject.width / 2) {
      if (!above || box.y > above.y) above = { id, y: box.y };
    }
  }
  if (sameRow) return { id: sameRow.id, key: "ArrowLeft" };
  if (above) return { id: above.id, key: "ArrowUp" };
  throw new Error(`workspace card ${subjectId} has no rendered predecessor to reorder against`);
}

/** Force the web client to refetch backend state. Entities created over the
 *  REST API don't always arrive via the live event stream, so we nudge the
 *  store's refresh hook and let SET_STATE re-render the sidebar. */
async function syncUi(page: Page) {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
}

const tabRow = (wsId: string, tabId: string) =>
  `div[data-dnd-group="tab:${wsId}"]:has(> [data-tab-id="${tabId}"])`;
const wsRow = (wsId: string) =>
  `div[data-dnd-group="workspace:workspaces"]:has(> [data-workspace-id="${wsId}"])`;

const ownedWorkspaces: string[] = [];

test.afterEach(async ({ api }) => {
  for (const id of ownedWorkspaces.splice(0)) {
    await api.closeWorkspace(id).catch(() => {});
  }
});

async function makeWorkspace(api: ApiClient, name: string): Promise<string> {
  const ws = await api.post<{ id: string }>("/workspaces", { name, activate: true });
  ownedWorkspaces.push(ws.id);
  return ws.id;
}
async function makeTab(api: ApiClient, wsId: string): Promise<{ id: string; tab_id: string }> {
  return api.post<{ id: string; tab_id: string }>(`/workspaces/${wsId}/tabs`, {});
}

test("@golden C1 a top-level tab reorders within its workspace", async ({ api, ui }) => {
  const { page } = ui;
  const ws = await makeWorkspace(api, `C1 reorder ${Date.now()}`);
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

/** NOT COVERED (gesture): the sidebar no longer renders a `child:<parentTabId>`
 *  drop group — `SortableGroupData.group` is only ever "workspace" or "tab" in
 *  src/components/Sidebar, and nested tabs render as ordinary rows. The
 *  group-scoped contract itself is still live in MultiplexerState::reorder_children
 *  / reorder_tabs, so it is asserted here through the routes the sidebar calls. */
test("@golden C2 a child reorders within its parent group and cannot leave it", async ({ api }) => {
  const ws = await makeWorkspace(api, `C2 hierarchy ${Date.now()}`);
  const parent = await makeTab(api, ws);
  const c1 = await makeTab(api, ws);
  const c2 = await makeTab(api, ws);
  await api.post(`/tabs/${c1.tab_id}/role`, { role: "Worker", parent_tab_id: parent.tab_id });
  await api.post(`/tabs/${c2.tab_id}/role`, { role: "Worker", parent_tab_id: parent.tab_id });

  const ownedChildren = [c1.tab_id, c2.tab_id];
  expect(onlyOwned(await childOrder(api, ws, parent.tab_id), ownedChildren)).toEqual(ownedChildren);

  // Positive: reorder the two siblings → order flips, both stay children.
  await api.post(`/tabs/${parent.tab_id}/children/reorder`, {
    ordered_tab_ids: [c2.tab_id, c1.tab_id],
  });
  expect(onlyOwned(await childOrder(api, ws, parent.tab_id), ownedChildren))
    .toEqual([c2.tab_id, c1.tab_id]);

  // Negative: the top-level group refuses to absorb a child.
  const topBefore = await topLevelOrder(api, ws);
  const escape = api.post(`/workspaces/${ws}/tabs/reorder`, {
    ordered_tab_ids: [c2.tab_id, ...topBefore],
  });
  await expect(escape, "the top-level op must reject a child tab").rejects.toThrow();

  const state = await muxState(api);
  expect(state.tabs[c2.tab_id]?.parent_tab_id, "child must not escape its parent group").toBe(parent.tab_id);
  expect(await topLevelOrder(api, ws), "top-level group must not absorb the child").toEqual(topBefore);
  expect(onlyOwned(await childOrder(api, ws, parent.tab_id), ownedChildren))
    .toEqual([c2.tab_id, c1.tab_id]);
});

test("@golden C3 workspaces reorder among themselves", async ({ api, ui }) => {
  const { page } = ui;
  const a = await makeWorkspace(api, `C3 alpha ${Date.now()}`);
  const b = await makeWorkspace(api, `C3 beta ${Date.now()}`);

  await syncUi(page);
  await page.locator(`[data-workspace-id="${b}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  const before = await ordinaryWorkspaceOrder(api);
  const ia = before.indexOf(a);
  const ib = before.indexOf(b);
  expect(ia).toBeGreaterThanOrEqual(0);
  expect(ib).toBeGreaterThan(ia);

  // Drag beta onto the card rendered before it → beta now precedes that card.
  const neighbour = await precedingCard(page, b, before.filter((id) => id !== b));
  expect(before.indexOf(neighbour.id), "the rendered predecessor sorts before beta").toBeLessThan(ib);
  await keyboardDrag(page, wsRow(b), neighbour.key, 1);
  await expect
    .poll(
      async () => {
        const order = await ordinaryWorkspaceOrder(api);
        return order.indexOf(b) < order.indexOf(neighbour.id);
      },
      { timeout: 15_000 },
    )
    .toBe(true);
});

test("@golden C4 reordered order survives a reload", async ({ api, ui }) => {
  const { page } = ui;
  const ws = await makeWorkspace(api, `C4 persist ${Date.now()}`);
  const t1 = await makeTab(api, ws);
  const t2 = await makeTab(api, ws);
  const t3 = await makeTab(api, ws);
  const owned = [t1.tab_id, t2.tab_id, t3.tab_id];

  await syncUi(page);
  await page.locator(`[data-tab-id="${t3.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  await keyboardDrag(page, tabRow(ws, t3.tab_id), "ArrowUp", 2);
  const reordered = [t3.tab_id, t1.tab_id, t2.tab_id];
  await expect
    .poll(async () => onlyOwned(await topLevelOrder(api, ws), owned).join(","), { timeout: 15_000 })
    .toBe(reordered.join(","));

  // Persist to disk, then reload the client and confirm the sidebar re-sorts.
  await api.post("/session/save");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(`[data-tab-id="${t1.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  const domOrder = await page.evaluate((wsId: string) =>
    Array.from(document.querySelectorAll(`div[data-dnd-group="tab:${wsId}"]`))
      .map((row) => row.querySelector("[data-tab-id]")?.getAttribute("data-tab-id"))
      .filter((id): id is string => Boolean(id)),
  ws);
  // Rendered top-level rows reflect the persisted order.
  expect(onlyOwned(domOrder, owned)).toEqual(reordered);
  expect(onlyOwned(await topLevelOrder(api, ws), owned)).toEqual(reordered);
});
