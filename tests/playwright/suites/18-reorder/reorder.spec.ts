/**
 * WHAT: reorder covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 18-reorder.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test, type Page } from "../../helpers/fixtures";

type Row = ReturnType<Page["locator"]>;
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

/** Sentinel for "the row itself holds focus" — anything else is the thief. */
const ROW = "the sortable row";

type ArrowKey = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

/** dnd-kit's screen-reader region, which announces the drop target on every
 *  `onDragOver`. It is the only per-step signal that an arrow key actually
 *  moved the drag rather than arriving before the rects were remeasured. */
const liveRegion = (page: Page) => page.locator('[id^="DndLiveRegion"]');

/** Who holds focus, described against the row itself. `toBeFocused()` can only
 *  answer yes/no, so a stolen-focus failure reads "inactive" and never names the
 *  thief; this reports the actual owner, which is the whole question. */
async function focusState(row: Row): Promise<string> {
  return row.evaluate((el, rowLabel) => {
    const active = document.activeElement as HTMLElement | null;
    if (active === el) return rowLabel;
    if (!active) return "nothing";
    const testId = active.getAttribute("data-testid");
    return [active.tagName.toLowerCase(), testId && `[${testId}]`, active.className && `.${active.className}`]
      .filter(Boolean)
      .join("")
      .slice(0, 120);
  }, ROW);
}

/** Keyboard drag a dnd-kit sortable row: focus, Space (lift), one arrow per
 *  entry in `overIds` (each confirmed by the announcement naming that id), then
 *  Space (drop). Confirming each step is what stops a swallowed press from
 *  landing the item one slot short and reading as a persistence bug. */
async function keyboardDrag(page: Page, rowSelector: string, key: ArrowKey, overIds: string[]) {
  const row = sortableRow(page, rowSelector);
  await row.waitFor({ state: "visible", timeout: 15_000 });
  // Keep focus on the row because dnd-kit activates from a synthetic keydown.
  await row.focus();
  await expect
    .poll(() => focusState(row), {
      timeout: 5_000,
      message: "the sortable row holds focus when the lift key is delivered",
    })
    .toBe(ROW);
  await row.press("Space");
  await expect(row).toHaveAttribute("data-dnd-dragging", "true", { timeout: 15_000 });
  for (const overId of overIds) {
    await row.press(key);
    const holder = await focusState(row);
    await expect(liveRegion(page), `the drag moved over ${overId}; focus was on ${holder}`)
      .toContainText(overId, { timeout: 10_000 });
  }
  await row.press("Space");
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

/** The sidebar is worker-scoped state: one page serves every test in the file.
 *  Establish the surfaces these tests read instead of inheriting whatever the
 *  previous test left, and fail on the missing surface rather than 20s later on
 *  a row inside it. */
async function readySidebar(page: Page) {
  await page.locator('[data-testid="sidebar"]').waitFor({ state: "visible", timeout: 30_000 });
  const workspaces = page.locator('[data-testid="workspaces-section-toggle"]');
  await workspaces.waitFor({ state: "visible", timeout: 30_000 });
  if ((await workspaces.getAttribute("aria-expanded")) !== "true") {
    await workspaces.click();
  }
  await expect(workspaces).toHaveAttribute("aria-expanded", "true");
}

/** A keyboard drag that fails mid-gesture leaves dnd-kit holding an active drag
 *  on the shared page, and a DndContext with an active drag will not start
 *  another — so one failing drag test silently disables every later one. Cancel
 *  it here, and assert the cancel worked: if Escape cannot clear it either, the
 *  context is wedged rather than merely mid-drag, which is worth knowing. */
async function cancelStuckDrag(page: Page) {
  const dragging = page.locator('[data-dnd-dragging="true"]');
  if ((await dragging.count()) === 0) return;
  await page.keyboard.press("Escape");
  await expect(dragging, "Escape must cancel a drag left in flight").toHaveCount(0, { timeout: 10_000 });
}

/** Address a sortable row by the card it wraps. The wrapper carries no id of
 *  its own, and a `:has(> …)` relative selector is the kind of thing worth not
 *  betting a whole cluster on. */
const tabRow = (wsId: string, tabId: string) =>
  `div[data-dnd-group="tab:${wsId}"] > [data-tab-id="${tabId}"]`;
const wsRow = (wsId: string) =>
  `div[data-dnd-group="workspace:workspaces"] > [data-workspace-id="${wsId}"]`;
const sortableRow = (page: Page, cardSelector: string) =>
  page.locator(cardSelector).first().locator("xpath=..");

const ownedWorkspaces: string[] = [];

test.afterEach(async ({ api, ui }) => {
  await cancelStuckDrag(ui.page).catch(() => {});
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

  await readySidebar(page);
  await syncUi(page);
  await page.locator(`[data-tab-id="${t3.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  const owned = [t1.tab_id, t2.tab_id, t3.tab_id];
  expect(onlyOwned(await topLevelOrder(api, ws), owned)).toEqual(owned);

  // Drag the first tab down two slots → it lands last.
  await keyboardDrag(page, tabRow(ws, t1.tab_id), "ArrowDown", [t2.tab_id, t3.tab_id]);

  await expect
    .poll(async () => onlyOwned(await topLevelOrder(api, ws), owned).join(","), { timeout: 15_000 })
    .toBe([t2.tab_id, t3.tab_id, t1.tab_id].join(","));
});


test("@golden C3 workspaces reorder among themselves", async ({ api, ui }) => {
  const { page } = ui;
  const a = await makeWorkspace(api, `C3 alpha ${Date.now()}`);
  const b = await makeWorkspace(api, `C3 beta ${Date.now()}`);

  await readySidebar(page);
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
  await keyboardDrag(page, wsRow(b), neighbour.key, [neighbour.id]);
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

  await readySidebar(page);
  await syncUi(page);
  await page.locator(`[data-tab-id="${t3.tab_id}"]`).first().waitFor({ state: "visible", timeout: 20_000 });
  await keyboardDrag(page, tabRow(ws, t3.tab_id), "ArrowUp", [t2.tab_id, t1.tab_id]);
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
