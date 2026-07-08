import { test, expect } from "../../helpers/fixtures";

// Regression oracle for the applyBackendState tab-bounce: web setActiveTab splits
// into two POSTs; POST#1 (workspace focus) emits a snapshot while active_tab is
// still the previous tab, and a stale refresh would revert the optimistic switch.
test("@golden web tab-switch does not bounce to previous tab", async ({ ui, api }) => {
  const page = ui.page;

  let st = await api.get<any>("/mux/state");
  let ws = st.workspaces.find((w: any) => w.tabs.length >= 2) ?? st.workspaces[0];
  if (!ws || ws.tabs.length < 2) {
    await api.post(`/workspaces/${ws.id}/tabs`, { eager: true });
    st = await api.get<any>("/mux/state");
    ws = st.workspaces.find((w: any) => w.id === ws.id);
  }
  const [tabA, tabB] = ws.tabs.slice(0, 2).map((t: any) => t.id);

  // force: dnd-kit stamps aria-disabled on the sortable row wrapper, which
  // trips Playwright's actionability check even though real clicks work.
  await page.locator(`[data-tab-id="${tabA}"]`).click({ force: true });
  await expect
    .poll(async () =>
      page
        .locator(`[data-tab-id="${tabA}"]`)
        .evaluate((e) => !e.className.includes("sidebar-tab-dimmed")),
    )
    .toBe(true);

  // Delay POST#2 (tab focus) so POST#1's stale workspace-focus refresh lands
  // first — deterministically forces the bounce window the guard must survive.
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
  await page.locator(`[data-tab-id="${tabB}"]`).click({ force: true });
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
