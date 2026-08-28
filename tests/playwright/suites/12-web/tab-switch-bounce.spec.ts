/**
 * WHAT: Web tab switching stays on the user-selected tab after split backend focus updates.
 * WHY: A stale mux refresh once reverted optimistic tab focus to the previously active tab.
 * ORACLE: The UI and backend converge on the newly selected tab after the delayed workspace-focus response lands.
 */
import { expect, test } from "../../helpers/fixtures";

type WorkspaceTab = { id: string; tab_id: string };

type MuxState = {
  active_workspace_id?: string | null;
  workspaces: Array<{ id: string; active_tab_id?: string | null; tabs: WorkspaceTab[] }>;
  tabs: Record<string, { hidden?: boolean; show_in_tabbar?: boolean }>;
};

function visibleTabs(
  workspace: MuxState["workspaces"][number],
  tabs: MuxState["tabs"],
): WorkspaceTab[] {
  return workspace.tabs.filter((entry) => {
    const tab = tabs[entry.tab_id];
    return tab && tab.hidden !== true && tab.show_in_tabbar !== false;
  });
}

// Regression oracle for applyBackendState tab-bounce: POST#1 can emit stale
// active_tab before POST#2, reverting the optimistic switch.
test("@golden web tab-switch does not bounce to previous tab", async ({ ui, api }) => {
  const page = ui.page;
  const created = await api.post<{ id?: string; workspace_id?: string }>("/workspaces", {
    name: `pw-bounce-${Date.now()}`,
    activate: true,
  });
  const workspaceId = created.id ?? created.workspace_id;
  expect(workspaceId, "bounce fixture workspace must be created").toBeTruthy();
  let tabBFocusRoute: string | undefined;

  try {
    await api.post(`/workspaces/${workspaceId}/tabs`, { eager: true });
    await api.post(`/workspaces/${workspaceId}/tabs`, { eager: true });
    await api.focusWorkspace(workspaceId!);

    const state = await api.get<MuxState>("/mux/state");
    const workspace = state.workspaces.find((candidate) => candidate.id === workspaceId);
    expect(workspace, "bounce fixture workspace must remain in mux state").toBeTruthy();
    expect(state.active_workspace_id, "bounce fixture workspace must be focused").toBe(workspaceId);
    const tabs = visibleTabs(workspace!, state.tabs);
    expect(tabs, "bounce fixture must expose exactly two selectable tabs").toHaveLength(2);
    const [tabA, tabB] = tabs;

    const tabRow = (tabId: string) =>
      page.locator(`[data-testid="tab-item"][data-tab-id="${tabId}"]`).first();
    const activeTab = (tabId: string) => tabRow(tabId).locator(".active-tab-pill");

    await expect(
      tabRow(tabA.id),
      "fixture tab A must render as selectable",
    ).toBeVisible({ timeout: 20_000 });
    await expect(
      tabRow(tabB.id),
      "fixture tab B must render as selectable",
    ).toBeVisible({ timeout: 20_000 });

    await api.post(`/workspaces/${workspaceId}/tabs/${tabA.id}/focus`);
    const focusedState = await api.get<MuxState>("/mux/state");
    expect(
      focusedState.workspaces.find((candidate) => candidate.id === workspaceId)?.active_tab_id,
      "bounce fixture backend must begin focused on tab A",
    ).toBe(tabA.id);
    await tabRow(tabA.id).click({ force: true });
    await expect(activeTab(tabA.id), "fixture must begin focused on tab A").toBeVisible({ timeout: 20_000 });

    let releaseTabFocus: (() => void) | undefined;
    const tabFocusReleased = new Promise<void>((resolve) => {
      releaseTabFocus = resolve;
    });
    let markTabFocusStarted: (() => void) | undefined;
    const tabFocusStarted = new Promise<void>((resolve) => {
      markTabFocusStarted = resolve;
    });
    const workspaceFocusResponse = page.waitForResponse((response) => {
      const request = response.request();
      return (
        request.method() === "POST" &&
        response.ok() &&
        new URL(response.url()).pathname === `/api/workspaces/${workspaceId}/focus`
      );
    });

    tabBFocusRoute = `**/api/workspaces/${workspaceId}/tabs/${tabB.id}/focus`;
    await page.route(tabBFocusRoute, async (route) => {
      markTabFocusStarted!();
      await tabFocusReleased;
      await route.continue();
    });

    await page.evaluate(() => {
      (window as any).__hist = [];
      const sample = () => {
        const el = document.querySelector("[data-tab-id]:not(.sidebar-tab-dimmed)");
        const id = el?.getAttribute("data-tab-id");
        if (id) (window as any).__hist.push(id);
      };
      const observer = new MutationObserver(sample);
      observer.observe(document.body, { attributes: true, subtree: true, attributeFilter: ["class"] });
      (window as any).__obsStop = () => observer.disconnect();
      sample();
    });
    await tabRow(tabB.id).click({ force: true });
    await tabFocusStarted;
    await workspaceFocusResponse;
    releaseTabFocus!();
    await expect(
      activeTab(tabB.id),
      "tab B must remain focused after stale workspace focus",
    ).toBeVisible({ timeout: 20_000 });
    await page.evaluate(() => (window as any).__obsStop());

    const history: string[] = await page.evaluate(() => (window as any).__hist);
    const firstB = history.indexOf(tabB.id);
    expect(firstB, `B must become active: ${history.join(">")}`).toBeGreaterThanOrEqual(0);
    expect(
      history.slice(firstB).includes(tabA.id),
      `bounce back to A: ${history.join(">")}`,
    ).toBe(false);
  } finally {
    if (tabBFocusRoute) await page.unroute(tabBFocusRoute).catch(() => {});
    await api.closeWorkspace(workspaceId!).catch(() => {});
  }
});
