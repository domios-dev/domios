/**
 * WHAT: pin general covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 18-reorder.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
// Sidebar.tsx partitions the protected workspace out of the reorderable group
// (partitionSidebarWorkspaces) and renders it in its own `general-section`
// outside SidebarDnd, so "General cannot be dragged" is now structural.
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

function workspaceTabs(state: MuxState, wsId: string): StateTab[] {
  const ws = state.workspaces.find((w) => w.id === wsId);
  if (!ws) return [];
  return ws.tabs.map((tabId) => state.tabs[tabId]).filter((tab): tab is StateTab => Boolean(tab));
}

/** The General workspace — the (single) protected workspace, created at boot by
 *  service/general_agent/workspace.rs. */
async function generalWorkspace(api: ApiClient): Promise<StateWs> {
  const state = await muxState(api);
  const ws = state.workspaces.find((w) => w.protected);
  if (!ws) throw new Error("no protected (General) workspace found");
  return ws;
}

/** The General tab — the protected tab of the protected workspace. */
async function generalTab(api: ApiClient, wsId: string): Promise<StateTab> {
  const state = await muxState(api);
  const tab = workspaceTabs(state, wsId).find((candidate) => candidate.protected);
  if (!tab) throw new Error("no protected (General) tab found");
  return tab;
}

/** Top-level tab ids of a workspace, in sort_order. */
async function topLevelOrder(api: ApiClient, wsId: string): Promise<string[]> {
  const state = await muxState(api);
  return workspaceTabs(state, wsId)
    .filter((tab) => tab.hidden !== true && !tab.parent_tab_id)
    .sort(bySortOrder)
    .map((tab) => tab.id);
}

/** Ordinary (non-protected) workspace ids in sort_order — exactly the group the
 *  card grid renders as sortable. */
async function ordinaryWorkspaceOrder(api: ApiClient): Promise<string[]> {
  const state = await muxState(api);
  return state.workspaces.filter((ws) => !ws.protected).sort(bySortOrder).map((ws) => ws.id);
}

type ArrowKey = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

async function keyboardDrag(page: Page, rowSelector: string, key: ArrowKey, presses: number) {
  const row = sortableRow(page, rowSelector);
  await row.waitFor({ state: "visible", timeout: 15_000 });
  // dnd-kit activates from a synthetic onKeyDown on this node, so the row must
  // still hold focus when the key lands — assert that separately from the lift.
  await row.focus();
  await expect(row, "the sortable row holds focus when the lift key is delivered").toBeFocused();
  await row.press("Space");
  await expect(row).toHaveAttribute("data-dnd-dragging", "true", { timeout: 15_000 });
  for (let i = 0; i < presses; i++) {
    await row.press(key);
  }
  await row.press("Space");
  await expect(row).not.toHaveAttribute("data-dnd-dragging", "true", { timeout: 15_000 });
}

/** Workspaces render as a card grid (rectSortingStrategy), so which card a
 *  single arrow reaches depends on where the cards actually landed, not on
 *  their index. Find the subject's rendered predecessor and the arrow onto it. */
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

async function syncUi(page: Page) {
  await page.evaluate(() => window.dispatchEvent(new Event("codeterm-refresh-mux-state")));
}

/** Address a sortable row by the card it wraps — the wrapper carries no id of
 *  its own, and a `:has(> …)` relative selector is worth not betting on. */
const tabRow = (wsId: string, tabId: string) =>
  `div[data-dnd-group="tab:${wsId}"] > [data-tab-id="${tabId}"]`;
const wsRow = (wsId: string) =>
  `div[data-dnd-group="workspace:workspaces"] > [data-workspace-id="${wsId}"]`;
const sortableRow = (page: Page, cardSelector: string) =>
  page.locator(cardSelector).first().locator("xpath=..");

const ownedWorkspaces: string[] = [];
const ownedTabs: string[] = [];

test.afterEach(async ({ api }) => {
  for (const id of ownedTabs.splice(0)) {
    await api.closeTab(id).catch(() => {});
  }
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
  const tab = await api.post<{ id: string; tab_id: string }>(`/workspaces/${wsId}/tabs`, {});
  ownedTabs.push(tab.tab_id);
  return tab;
}

test("@golden P1 dragging a workspace to the top cannot place it above General", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspace(api);
  const a = await makeWorkspace(api, `P1 alpha ${Date.now()}`);
  const b = await makeWorkspace(api, `P1 beta ${Date.now()}`);

  await syncUi(page);
  await page.locator(`[data-workspace-id="${b}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  // General is not a member of the reorderable group at all, so no drop can
  // target a slot above it.
  await expect(page.locator(wsRow(general.id))).toHaveCount(0);
  await expect(page.locator('[data-testid="general-section"]')).toBeVisible();

  const before = await ordinaryWorkspaceOrder(api);
  expect(before.indexOf(a)).toBeGreaterThanOrEqual(0);
  expect(before.indexOf(b)).toBeGreaterThan(before.indexOf(a));

  // Drag beta up onto the card rendered before it — the highest slot a single
  // step can reach inside the group General is not a member of.
  const neighbour = await precedingCard(page, b, before.filter((id) => id !== b));
  await keyboardDrag(page, wsRow(b), neighbour.key, 1);

  // The drag really committed (this is what stops the guard being vacuous)…
  await expect
    .poll(
      async () => {
        const order = await ordinaryWorkspaceOrder(api);
        return order.indexOf(b) < order.indexOf(neighbour.id);
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  // …and General is still pinned at the lowest ordinal, outside the group.
  const after = await muxState(api);
  const generalAfter = after.workspaces.find((ws) => ws.id === general.id);
  expect(generalAfter?.sort_order ?? -1, "General stays pinned at ordinal 0").toBe(0);
  expect(
    Math.min(...after.workspaces.map((ws) => ws.sort_order ?? 0)),
    "no workspace sorts above General",
  ).toBe(0);
  await expect(page.locator(wsRow(general.id)), "General never joins the sortable group").toHaveCount(0);
});

/** NOT COVERED (gesture): the General section renders outside `SidebarDnd`
 *  (Sidebar.tsx), so no DndContext drives its rows and a keyboard drag there is
 *  inert — asserting "the drop was rejected" would pass without exercising
 *  anything. The clamp itself is live in MultiplexerState::reorder_tabs
 *  (pin_protected_tabs_first) and is asserted through the route the sidebar calls. */
test("@golden P2 dragging a tab to the top cannot place it above the General tab", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspace(api);
  const gTab = await generalTab(api, general.id);
  const t1 = await makeTab(api, general.id);
  const t2 = await makeTab(api, general.id);

  await syncUi(page);
  await page.locator(`[data-tab-id="${t2.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  const before = await topLevelOrder(api, general.id);
  expect(before[0], "General tab starts pinned at the top").toBe(gTab.id);
  expect(before).toContain(t2.tab_id);

  // Ask for an order that puts the newest tab first — the daemon clamps the
  // protected tab back to index 0 and keeps the rest in the requested order.
  const requested = [t2.tab_id, ...before.filter((id) => id !== t2.tab_id)];
  await api.post(`/workspaces/${general.id}/tabs/reorder`, { ordered_tab_ids: requested });

  const after = await topLevelOrder(api, general.id);
  expect(after[0], "General tab stays at index 0").toBe(gTab.id);
  expect(after.indexOf(t2.tab_id), "the moved tab never rises above the General tab").toBeGreaterThan(0);
  expect(after, "the rest keep the requested relative order below General")
    .toEqual([gTab.id, ...requested.filter((id) => id !== gTab.id)]);

  // The rendered sidebar agrees with the clamped backend order.
  await syncUi(page);
  await expect
    .poll(
      async () =>
        page.evaluate((wsId: string) =>
          Array.from(document.querySelectorAll(`div[data-dnd-group="tab:${wsId}"]`))
            .map((row) => row.querySelector("[data-tab-id]")?.getAttribute("data-tab-id"))
            .filter((id): id is string => Boolean(id))[0],
        general.id),
      { timeout: 15_000 },
    )
    .toBe(gTab.id);
});

test("@golden P3 the General workspace and General tab are not draggable", async ({ api, ui }) => {
  const { page } = ui;
  const general = await generalWorkspace(api);
  const gTab = await generalTab(api, general.id);
  // A sibling below General, so the sortable group is non-empty.
  const sibling = await makeWorkspace(api, `P3 sibling ${Date.now()}`);

  await syncUi(page);
  await page.locator(`[data-workspace-id="${sibling}"]`).first().waitFor({ state: "visible", timeout: 20_000 });

  // The General workspace has no sortable row — it renders in its own section.
  await expect(page.locator(wsRow(general.id))).toHaveCount(0);
  const generalSection = page.locator('[data-testid="general-section"]');
  await expect(generalSection).toBeVisible();
  await expect(generalSection.locator(`[data-tab-id="${gTab.id}"]`)).toBeVisible();

  // The General tab's row exists but carries dnd-kit's disabled affordance.
  // dnd-kit stamps aria-disabled on the sortable wrapper, not on the card.
  await expect(sortableRow(page, tabRow(general.id, gTab.id))).toHaveAttribute("aria-disabled", "true");

  // The daemon refuses to demote either one, whatever order is asked for.
  const wsBefore = await ordinaryWorkspaceOrder(api);
  await api.post("/workspaces/reorder", {
    ordered_workspace_ids: [...wsBefore, general.id],
  });
  const wsAfter = await muxState(api);
  expect(wsAfter.workspaces.find((ws) => ws.id === general.id)?.sort_order, "General workspace did not move")
    .toBe(0);

  const tabsBefore = await topLevelOrder(api, general.id);
  await api.post(`/workspaces/${general.id}/tabs/reorder`, {
    ordered_tab_ids: [...tabsBefore.filter((id) => id !== gTab.id), gTab.id],
  });
  expect(await topLevelOrder(api, general.id), "General tab did not move").toEqual(tabsBefore);
});
