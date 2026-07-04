/**
 * 20-new-session — the new-session picker (task_234c14f0) and the
 * orchestration-pair in-place spawn + sidebar group it drives (task_e0ddee08).
 *
 * Real dev console, web-mode (@golden). Each scenario opens its own fresh
 * picker via beforeEach; scenarios that actually spawn agents (7-10) clean up
 * their panes/workspaces in afterEach so a failed run never leaks live
 * `claude` processes.
 */
import { expect, test, type Page } from "../../helpers/fixtures";
import { type ApiClient } from "../../helpers/api-client";

const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";

interface MuxTab {
  id: string;
  pane_id: string;
}
interface MuxWorkspace {
  id: string;
  name: string;
  tabs: MuxTab[];
}
interface MuxPane {
  id: string;
  role?: string | null;
  parent_pane_id?: string | null;
  agent_type?: string | null;
  title: string;
}
interface MuxState {
  workspaces: MuxWorkspace[];
  panes: Record<string, MuxPane>;
}

// The dev API can go briefly unresponsive while a real agent PTY is booting
// (spawning is synchronous work on the same request path — see AGENTS.md's
// hot-path note) long enough to eat the client's 15s abort timeout in one
// shot. A bare, unpolled call right after such a spawn has no chance to
// retry, so give every /mux/state read a couple of quiet retries.
async function muxState(api: ApiClient): Promise<MuxState> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      return await api.get<MuxState>("/mux/state");
    } catch (e) {
      lastError = e;
    }
  }
  throw lastError;
}

async function createTestWorkspace(api: ApiClient, prefix: string): Promise<{ id: string; name: string }> {
  const name = `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  await api.createWorkspace(name);
  const ws = (await api.workspaces()).find((w) => w.name === name);
  if (!ws) throw new Error(`workspace ${name} missing after create`);
  return { id: ws.id, name };
}

// WorkspaceItem (the header row) and SidebarWorkspaceTabs (name, tabs, "New
// session") are SIBLINGS, not parent/child — both only share the outer
// `workspace-group` wrapper, so that's the scope for anything below the header.
function workspaceItem(page: Page, wsId: string) {
  return page.locator(`[data-testid="workspace-item"][data-workspace-id="${wsId}"]`);
}
function workspaceGroup(page: Page, wsId: string) {
  return page.locator(`[data-testid="workspace-group"][data-workspace-id="${wsId}"]`);
}

async function expandWorkspace(page: Page, wsId: string): Promise<void> {
  const item = workspaceItem(page, wsId);
  await expect(item).toBeVisible({ timeout: 15_000 });
  const expand = item.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) await expand.click();
}

/** Clicks "New session" inside the given workspace and returns the fresh
 *  picker tab id — found by diffing the workspace's tab list before/after. */
async function openPicker(api: ApiClient, page: Page, wsId: string): Promise<string> {
  await expandWorkspace(page, wsId);
  const before = new Set((await muxState(api)).workspaces.find((w) => w.id === wsId)?.tabs.map((t) => t.id) ?? []);
  await workspaceGroup(page, wsId).getByText("New session", { exact: true }).click();
  await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
  const after = (await muxState(api)).workspaces.find((w) => w.id === wsId)?.tabs ?? [];
  const fresh = after.find((t) => !before.has(t.id));
  if (!fresh) throw new Error(`no new picker tab appeared in workspace ${wsId}`);
  return fresh.id;
}

async function waitForPairHead(api: ApiClient, paneId: string): Promise<MuxPane> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const pane = (await muxState(api)).panes[paneId];
    if (pane?.role === "Orchestrator" && pane.agent_type) return pane;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`pane ${paneId} did not become an orchestrator agent`);
}

async function cleanupWorkspace(api: ApiClient, wsId: string | undefined): Promise<void> {
  if (!wsId) return;
  await api.closeWorkspace(wsId).catch(() => {});
}

test.describe("new-session picker", () => {
  // The golden project defaults to a 300s timeout — fine for the 4 scenarios
  // that spawn real agents (they raise it explicitly), but it means a broken
  // beforeEach on the other 6 burns 5 minutes before surfacing. Cut the
  // default so a real failure shows up in seconds, not minutes.
  test.describe.configure({ timeout: 30_000 });

  let ws: { id: string; name: string };

  test.beforeEach(async ({ api, ui }) => {
    ws = await createTestWorkspace(api, "pw-picker");
    await openPicker(api, ui.page, ws.id);
  });

  test.afterEach(async ({ api }) => {
    await cleanupWorkspace(api, ws?.id);
  });

  test("@golden OPEN: New session shows the picker, spawns nothing yet", async ({ api }) => {
    const panesBefore = await api.panes();
    const state = await muxState(api);
    const wsNow = state.workspaces.find((w) => w.id === ws.id);
    // Exactly the picker's own blank tab — no agent spawned.
    expect(wsNow?.tabs.length, "workspace should only have the picker's blank tab").toBe(1);
    const pane = state.panes[wsNow!.tabs[0].pane_id];
    expect(pane?.agent_type ?? null, "picker pane must have no agent yet").toBeNull();
    expect((await api.panes()).length).toBe(panesBefore.length);
  });

  test("@golden MODE CARDS: exactly one card selected at a time, pair-affordance follows it", async ({ ui }) => {
    const page = ui.page;
    const single = page.locator('[data-testid="picker-mode-single"]');
    const pair = page.locator('[data-testid="picker-mode-orchestration"]');
    await expect(single).toBeVisible();
    await expect(pair).toBeVisible();

    await pair.click();
    await expect(pair).toHaveAttribute("data-selected", "true");
    await expect(single).toHaveAttribute("data-selected", "false");
    await expect(page.locator('[data-testid="pair-affordance"]')).toBeVisible();

    await single.click();
    await expect(single).toHaveAttribute("data-selected", "true");
    await expect(pair).toHaveAttribute("data-selected", "false");
  });

  test("@golden DEFAULTS: orchestrator=claude/Opus, planner=claude/Fable with no prior persistence", async ({ ui }) => {
    const page = ui.page;
    await page.evaluate((key) => window.localStorage.removeItem(key), NEW_SESSION_CHOICE_STORAGE_KEY);
    await page.reload();
    await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await expect(page.locator('[data-testid="picker-provider-orchestrator"]')).toHaveValue("claude");
    await expect(page.locator('[data-testid="picker-provider-planner"]')).toHaveValue("claude");
    const orchModel = await page.locator('[data-testid="picker-model-orchestrator"]').inputValue();
    const plannerModel = await page.locator('[data-testid="picker-model-planner"]').inputValue();
    expect(orchModel.toLowerCase()).toContain("opus");
    expect(plannerModel.toLowerCase()).toContain("fable");
  });

  test("@golden WORKSPACE ROW: preselected to current, help tooltip is instant", async ({ ui }) => {
    const page = ui.page;
    await expect(page.locator('[data-testid="picker-workspace"]')).toHaveValue(ws.id);

    const help = page.locator('[data-testid="workspace-help"]');
    await help.hover();
    const tooltip = page.locator('[data-testid="workspace-help-tooltip"]');
    await expect(tooltip).toBeVisible({ timeout: 100 });
    await expect(tooltip).toContainText(/workspace/i);
  });

  test("@golden PREVIEW LINE: reflects mode + destination, updates live on workspace change", async ({ api, ui }) => {
    const page = ui.page;
    const otherWs = await createTestWorkspace(api, "pw-picker-preview-dest");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      const preview = page.locator('[data-testid="picker-preview"]');
      await expect(preview).toContainText("Orchestrator + Planner");
      await expect(preview).toContainText(ws.name);

      await page.locator('[data-testid="picker-workspace"]').selectOption(otherWs.id);
      await expect(preview).toContainText(otherWs.name);
      await expect(preview).not.toContainText(ws.name);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden PERSISTENCE: mode + model persist, workspace resets to current", async ({ api, ui }) => {
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-model-planner"]').selectOption({ index: 0 });
    const changedModel = await page.locator('[data-testid="picker-model-planner"]').inputValue();
    const otherWs = await createTestWorkspace(api, "pw-picker-persist-dest");
    try {
      await page.locator('[data-testid="picker-workspace"]').selectOption(otherWs.id);

      // Close this picker tab without spawning, then reopen a fresh one in
      // the ORIGINAL workspace (persistence must not follow the tab, only
      // the localStorage choice).
      const state = await muxState(api);
      const tab = state.workspaces.find((w) => w.id === ws.id)?.tabs[0];
      if (tab) await api.closePane(tab.pane_id).catch(() => {});

      await openPicker(api, page, ws.id);
      await expect(page.locator('[data-testid="picker-mode-orchestration"]')).toHaveAttribute("data-selected", "true");
      await expect(page.locator('[data-testid="picker-model-planner"]')).toHaveValue(changedModel);
      await expect(page.locator('[data-testid="picker-workspace"]')).toHaveValue(ws.id);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden IN-PLACE PAIR SPAWN: same tab hosts the orchestrator, planner is the only new tab", async ({ api, ui }) => {
    test.setTimeout(180_000);
    const page = ui.page;
    const before = await muxState(api);
    const pickerTab = before.workspaces.find((w) => w.id === ws.id)!.tabs[0];
    const tabCountBefore = before.workspaces.find((w) => w.id === ws.id)!.tabs.length;

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.tabs.length, {
        timeout: 60_000,
      })
      .toBeGreaterThanOrEqual(2);

    const after = await muxState(api);
    const wsAfter = after.workspaces.find((w) => w.id === ws.id)!;
    expect(wsAfter.tabs.some((t) => t.id === pickerTab.id), "picker tab id must survive unchanged").toBe(true);
    const orchestratorPane = after.panes[wsAfter.tabs.find((t) => t.id === pickerTab.id)!.pane_id];
    expect(orchestratorPane.role).toBe("Orchestrator");
    expect(orchestratorPane.id).toBe(pickerTab.pane_id);
    // Exactly one net new tab (the Planner) — no orphan empty tab, no extra objects.
    expect(wsAfter.tabs.length).toBe(tabCountBefore + 1);

    // Sidebar renders the group with the Planner as the pinned first child.
    await expect(page.locator(`[data-tab-id="${pickerTab.id}"]`)).toBeVisible({ timeout: 15_000 });
  });

  test("@golden GREETING: orchestrator's first assistant message is the PEER_ORCH_GREETING, unprompted", async ({ api, ui }) => {
    test.setTimeout(180_000);
    const page = ui.page;
    const before = await muxState(api);
    const pickerTab = before.workspaces.find((w) => w.id === ws.id)!.tabs[0];

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();
    await waitForPairHead(api, pickerTab.pane_id);

    await expect
      .poll(async () => {
        try {
          const res = await api.post<{ messages?: { type?: string; content?: string }[] }>(
            `/panes/${pickerTab.pane_id}/chat-raw/start`,
          );
          return res.messages?.[0]?.content ?? "";
        } catch {
          return "";
        }
      }, { timeout: 60_000 })
      .toContain("Tell me the goal");
  });

  test("@golden CROSS-WORKSPACE: spawning into workspace B leaves the picker tab in A alone", async ({ api, ui }) => {
    test.setTimeout(180_000);
    const page = ui.page;
    const wsB = await createTestWorkspace(api, "pw-picker-cross-b");
    try {
      const before = await muxState(api);
      const pickerTab = before.workspaces.find((w) => w.id === ws.id)!.tabs[0];

      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      await page.locator('[data-testid="picker-workspace"]').selectOption(wsB.id);
      await page.locator('[data-testid="picker-confirm"]').click();

      await expect
        .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === wsB.id)?.tabs.length, {
          timeout: 60_000,
        })
        .toBeGreaterThanOrEqual(2);

      // The picker's own tab in A never transforms — it closes normally
      // instead, via a fire-and-forget backend call. The API can itself be
      // momentarily slow while 2 real agent PTYs are booting (a single
      // /mux/state call can eat the whole 15s client timeout) — give this
      // real headroom so a slow poll attempt doesn't exhaust the budget.
      await expect
        .poll(
          async () => {
            const s = await muxState(api);
            return s.workspaces.find((w) => w.id === ws.id)?.tabs.some((t) => t.id === pickerTab.id) ?? false;
          },
          { timeout: 60_000 },
        )
        .toBe(false);

      const after = await muxState(api);
      const wsBAfter = after.workspaces.find((w) => w.id === wsB.id)!;
      const orchestrator = Object.values(after.panes).find(
        (p) => wsBAfter.tabs.some((t) => t.pane_id === p.id) && p.role === "Orchestrator",
      );
      expect(orchestrator, "orchestrator must land in workspace B").toBeDefined();
    } finally {
      await cleanupWorkspace(api, wsB.id);
    }
  });

  test("@golden GROUP CLOSE: children have no individual close; head close confirms and takes the whole group", async ({ api, ui }) => {
    test.setTimeout(180_000);
    const page = ui.page;
    const before = await muxState(api);
    const pickerTab = before.workspaces.find((w) => w.id === ws.id)!.tabs[0];

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.tabs.length, {
        timeout: 60_000,
      })
      .toBeGreaterThanOrEqual(2);

    const state = await muxState(api);
    const wsNow = state.workspaces.find((w) => w.id === ws.id)!;
    const plannerTab = wsNow.tabs.find((t) => t.id !== pickerTab.id)!;

    // Planner (child) row: open its menu, no enabled close action.
    const plannerRow = page.locator(`[data-tab-id="${plannerTab.id}"]`);
    await plannerRow.hover();
    await plannerRow.locator('button[title="Tab actions"]').click();
    await expect(page.locator('[data-testid="tab-menu-close"]')).toHaveCount(0);
    await page.keyboard.press("Escape");
    await page.mouse.click(2, 2);
    await expect(page.locator('[data-testid="tab-menu-close"]')).toHaveCount(0);

    // Head (orchestrator) row: close asks for confirmation listing the group.
    const headRow = page.locator(`[data-tab-id="${pickerTab.id}"]`);
    await expect(headRow).toBeVisible({ timeout: 15_000 });
    await headRow.hover();
    await headRow.locator('button[title="Tab actions"]').click();
    const closeItem = page.locator('[data-testid="tab-menu-close"]');
    await expect(closeItem).toBeVisible({ timeout: 5_000 });
    await closeItem.click();

    const dialog = page.locator('[data-testid="group-close-confirm"]');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText("Orchestrator");

    await page.locator('[data-testid="group-close-confirm-button"]').click();

    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.tabs.length ?? 0, {
        timeout: 60_000,
      })
      .toBe(0);
  });
});
