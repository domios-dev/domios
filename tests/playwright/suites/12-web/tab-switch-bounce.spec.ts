/**
 * WHAT: Web tab switching stays on the user-selected tab after split backend focus updates.
 * WHY: A stale mux refresh once reverted optimistic tab focus to the previously active tab.
 * ORACLE: The UI and backend converge on the newly selected tab after the delayed workspace-focus response lands.
 */
import { test, expect } from "../../helpers/fixtures";

function visibleTabs(ws: any, panes: Record<string, any>) {
  return ws.tabs.filter((tab: any) => {
    const pane = panes[tab.pane_id];
    return pane && pane.hidden !== true && pane.show_in_tabbar !== false;
  });
}

// Regression oracle for applyBackendState tab-bounce: POST#1 can emit stale
// active_tab before POST#2, reverting the optimistic switch.
test("@golden web tab-switch does not bounce to previous tab", async ({ ui, api }) => {
  const page = ui.page;

  let st = await api.get<any>("/mux/state");
  let ws =
    st.workspaces.find((w: any) => visibleTabs(w, st.panes).length >= 2) ??
    st.workspaces[0];
  for (let attempt = 0; ws && visibleTabs(ws, st.panes).length < 2 && attempt < 3; attempt++) {
    await api.post(`/workspaces/${ws.id}/tabs`, { eager: true });
    st = await api.get<any>("/mux/state");
    ws = st.workspaces.find((w: any) => w.id === ws.id);
  }
  if (!ws || visibleTabs(ws, st.panes).length < 2) {
    throw new Error("expected at least two sidebar-visible tabs for bounce regression");
  }
  const [tabA, tabB] = visibleTabs(ws, st.panes).slice(0, 2).map((t: any) => t.id);

  await expect(page.locator(`[data-testid="tab-item"][data-tab-id="${tabA}"]`).first()).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.locator(`[data-testid="tab-item"][data-tab-id="${tabB}"]`).first()).toBeVisible({
    timeout: 20_000,
  });

  // force: dnd-kit stamps aria-disabled on the sortable row wrapper, which
  // trips Playwright's actionability check even though real clicks work.
  await page.locator(`[data-testid="tab-item"][data-tab-id="${tabA}"]`).first().click({ force: true });
  await expect
    .poll(async () =>
      page
        .locator(`[data-testid="tab-item"][data-tab-id="${tabA}"]`)
        .first()
        .evaluate((e) => !e.className.includes("sidebar-tab-dimmed")),
    )
    .toBe(true);

  // Delay POST#2 so POST#1's stale workspace-focus refresh lands first.
  await page.route("**/tabs/*/focus", async (r) => {
    await new Promise((res) => setTimeout(res, 150));
    await r.continue();
  });

  await page.evaluate(() => {
    (window as any).__hist = [];
    const sample = () => {
      const el = document.querySelector("[data-tab-id]:not(.sidebar-tab-dimmed)");
      const id = el?.getAttribute("data-tab-id");
      if (id) (window as any).__hist.push(id);
    };
    const obs = new MutationObserver(sample);
    obs.observe(document.body, { attributes: true, subtree: true, attributeFilter: ["class"] });
    (window as any).__obsStop = () => obs.disconnect();
    sample();
  });
  await page.locator(`[data-testid="tab-item"][data-tab-id="${tabB}"]`).first().click({ force: true });
  // PW_JUSTIFIED_SLEEP: fixed 700ms sampling/settle window; no app event exists for this probe.
  await page.waitForTimeout(700);
  await page.evaluate(() => (window as any).__obsStop());

  const hist: string[] = await page.evaluate(() => (window as any).__hist);
  const firstB = hist.indexOf(tabB);
  expect(firstB, `B must become active: ${hist.join(">")}`).toBeGreaterThanOrEqual(0);
  expect(
    hist.slice(firstB).includes(tabA),
    `bounce back to A: ${hist.join(">")}`,
  ).toBe(false);
});
