/**
 * WHAT: picker covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 20-new-session.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "../../helpers/fixtures";
import { isStubTransportActive } from "../../helpers/stub-gate";
import { ApiClient, type Tab } from "../../helpers/api-client";
import { PlanRegistry } from "../../helpers/plan-cleanup";

const TEST_WS_PREFIX = "pw-picker";

const NEW_SESSION_CHOICE_STORAGE_KEY = "codeterm:new-session-choice";

interface MuxTab {
  id: string;
  tab_id: string;
}
interface MuxWorkspace {
  id: string;
  name: string;
  tabs: MuxTab[];
  active_tab_id?: string | null;
}
interface MuxTab {
  id: string;
  role?: string | null;
  parent_tab_id?: string | null;
  agent_type?: string | null;
  title: string;
  hidden?: boolean;
  show_in_tabbar?: boolean;
}
interface MuxState {
  workspaces: MuxWorkspace[];
  tabs: Record<string, MuxTab>;
  active_workspace_id?: string | null;
  focused_tab_id?: string | null;
}

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

/** Opens the VIRTUAL picker; no backend tab/PTY exists until confirm. */
async function openPicker(page: Page, wsId: string): Promise<void> {
  await expandWorkspace(page, wsId);
  await workspaceGroup(page, wsId).getByText("New session", { exact: true }).click();
  await expect(page.locator('[data-testid="new-session-picker"]')).toBeVisible({ timeout: 10_000 });
}

/** A shared DropdownSelect trigger — replaces the old native <select>. */
function dropdown(page: Page, testId: string) {
  return page.locator(`[data-testid="${testId}"]`);
}
/** Opens a DropdownSelect and clicks the option carrying `value`. */
async function selectDropdown(page: Page, testId: string, value: string): Promise<void> {
  await dropdown(page, testId).click();
  await page.locator(`[data-testid="${testId}-option-${value}"]`).click();
}
/** Opens a DropdownSelect and clicks the first option whose value differs from
 *  the current one; returns the value it landed on. */
async function selectFirstDifferentOption(page: Page, testId: string): Promise<string> {
  const current = await dropdown(page, testId).getAttribute("data-value");
  await dropdown(page, testId).click();
  const opts = page.locator(`[data-testid^="${testId}-option-"]`);
  const count = await opts.count();
  const prefix = `${testId}-option-`;
  for (let i = 0; i < count; i++) {
    const tid = await opts.nth(i).getAttribute("data-testid");
    const val = tid?.slice(prefix.length) ?? "";
    if (val && val !== current) {
      await opts.nth(i).click();
      return val;
    }
  }
  await page.keyboard.press("Escape");
  return current ?? "";
}

function tabCount(tabs: Tab[]): number {
  return tabs.length;
}
function ptyCount(tabs: Tab[]): number {
  return tabs.filter((p) => p.pty_id != null).length;
}

function visibleTabsInWorkspace(state: MuxState, wsId: string): MuxTab[] {
  const ws = state.workspaces.find((w) => w.id === wsId);
  return (ws?.tabs ?? []).filter((tab) => {
    const tab = state.tabs[tab.tab_id];
    return tab && tab.hidden !== true && tab.show_in_tabbar !== false;
  });
}

/** Reads PEER_ORCH_GREETING straight from the Rust source and splits it on the
 *  `{planner}` placeholder, so greeting assertions track Puccini's wording
 *  revision at the source instead of pinning a literal (round-18 addendum). */
function repoFile(rel: string): string {
  let dir = process.cwd();
  for (let i = 0; i < 8; i++) {
    const candidate = path.join(dir, rel);
    if (existsSync(candidate)) return candidate;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(`could not locate ${rel} from ${process.cwd()}`);
}
function peerOrchGreetingSegments(): string[] {
  const src = readFileSync(repoFile("src-tauri/src/prompts.rs"), "utf8");
  const m = src.match(/PEER_ORCH_GREETING\s*:\s*&str\s*=\s*"([\s\S]*?)"\s*;/);
  if (!m) throw new Error("PEER_ORCH_GREETING not found in prompts.rs");
  const raw = m[1].replace(/\\\s+/g, ""); // undo Rust string line-continuations
  return raw
    .split("{planner}")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

async function waitForOrchestrator(api: ApiClient, wsId: string): Promise<{ tab: MuxTab; tab: MuxTab }> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const s = await muxState(api);
    const ws = s.workspaces.find((w) => w.id === wsId);
    const tab = ws?.tabs.find((t) => s.tabs[t.tab_id]?.role === "Orchestrator");
    if (tab && s.tabs[tab.tab_id]?.agent_type) return { tab, tab: s.tabs[tab.tab_id] };
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`no orchestrator agent materialized in workspace ${wsId}`);
}

async function waitForSingleAgent(api: ApiClient, wsId: string): Promise<{ tab: MuxTab; tab: MuxTab }> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const s = await muxState(api);
    const ws = s.workspaces.find((w) => w.id === wsId);
    const tab = ws?.tabs.find((t) => s.tabs[t.tab_id]?.agent_type);
    if (tab) return { tab, tab: s.tabs[tab.tab_id] };
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`no single agent materialized in workspace ${wsId}`);
}

/** All sidebar tab titles inside a workspace group — from each tab's `title`
 *  attribute (first line; the tooltip is `title\ndescription`). */
async function sidebarTabTitles(page: Page, wsId: string): Promise<string[]> {
  const items = workspaceGroup(page, wsId).locator('[data-testid="tab-item"]');
  const count = await items.count();
  const titles: string[] = [];
  for (let i = 0; i < count; i++) {
    const t = (await items.nth(i).getAttribute("title")) ?? "";
    titles.push(t.split("\n")[0]);
  }
  return titles;
}
function isPlaceholderTitle(title: string): boolean {
  const t = title.trim();
  return t.length === 0 || t === "…" || t === "...";
}

async function cleanupWorkspace(api: ApiClient, wsId: string | undefined): Promise<void> {
  if (!wsId) return;
  await api.closeWorkspace(wsId).catch(() => {});
}

test.describe("new-session picker", () => {
  // Spawn-flow tests run on the stub transport; picker UI plumbing is
  // provider-independent, so golden carries no live-LLM cost here.
  async function requireStub(api: import("../../helpers/api-client").ApiClient) {
    test.skip(!(await isStubTransportActive(api)), "dev daemon is not running with CODETERM_STUB_AGENT_CMD");
  }

  // scenarios that spawn real agents raise their own timeout explicitly.
  test.describe.configure({ timeout: 30_000 });

  let ws: { id: string; name: string };
  // Id-diff registry catches bare "Orchestration"/timestamp title regressions.
  // Closing a workspace does not cascade to plans, so spawn scenarios need this.
  const plans = new PlanRegistry();

  test.beforeAll(async () => {
    await plans.snapshot();
  });

  test.beforeEach(async ({ api, ui }) => {
    ws = await createTestWorkspace(api, TEST_WS_PREFIX);
    await openPicker(ui.page, ws.id);
  });

  test.afterEach(async ({ api }) => {
    // Sweep created plans BEFORE closing the workspace (close doesn't cascade).
    // Runs even when the test body threw — teardown is not gated on asserts.
    await plans.sweep();
    await cleanupWorkspace(api, ws?.id);
  });

  test.afterAll(async () => {
    await plans.sweep();
    const leaked = await plans.created();
    expect(
      leaked,
      `suite must leave ZERO created plans (before == after). Leaked: ${leaked
        .map((p) => `${p.plan_id}:${JSON.stringify(p.title)}`)
        .join(", ")}`,
    ).toHaveLength(0);
    const junk = await plans.junk();
    expect(
      junk,
      `no orchmatrix-*/bare 'Orchestration'/timestamp junk may remain. Found: ${junk
        .map((p) => `${p.plan_id}:${JSON.stringify(p.title)}`)
        .join(", ")}`,
    ).toHaveLength(0);
  });

  test("@golden OPEN: New session shows the VIRTUAL picker, no backend tab/PTY", async ({ api, ui }) => {
    const tabs = await api.tabs();
    const state = await muxState(api);
    expect(visibleTabsInWorkspace(state, ws.id), "picker must not create a rendered backend tab").toHaveLength(0);
    // …and no snapshot tab carries the virtual picker's title.
    expect(
      Object.values(state.tabs).some((p) => p.title === "New session"),
      "no picker tab may appear in the snapshot",
    ).toBe(false);
    // The picker's own tab in the sidebar is titled, never a "…"/empty husk.
    for (const title of await sidebarTabTitles(ui.page, ws.id).catch(() => [])) {
      expect(isPlaceholderTitle(title), `sidebar tab title "${title}" must not be a placeholder`).toBe(false);
    }
    // Sanity: the picker tab is virtual, so /tabs never lists it.
    expect(tabs.some((p) => p.title === "New session")).toBe(false);
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
    await openPicker(page, ws.id);
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await expect(dropdown(page, "picker-provider-orchestrator")).toHaveAttribute("data-value", "claude");
    await expect(dropdown(page, "picker-provider-planner")).toHaveAttribute("data-value", "claude");
    const orchModel = (await dropdown(page, "picker-model-orchestrator").getAttribute("data-value")) ?? "";
    const plannerModel = (await dropdown(page, "picker-model-planner").getAttribute("data-value")) ?? "";
    expect(orchModel.toLowerCase()).toContain("opus");
    expect(plannerModel.toLowerCase()).toContain("fable");
  });

  test("@golden WORKSPACE ROW: preselected to current, help tooltip is instant", async ({ ui }) => {
    const page = ui.page;
    await expect(dropdown(page, "picker-workspace")).toHaveAttribute("data-value", ws.id);

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

      await selectDropdown(page, "picker-workspace", otherWs.id);
      await expect(preview).toContainText(otherWs.name);
      await expect(preview).not.toContainText(ws.name);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden PERSISTENCE: mode + model persist, workspace resets to current", async ({ api, ui }) => {
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    const changedModel = await selectFirstDifferentOption(page, "picker-model-planner");
    const otherWs = await createTestWorkspace(api, "pw-picker-persist-dest");
    try {
      await selectDropdown(page, "picker-workspace", otherWs.id);

      // localStorage choice; reopen a fresh picker in the ORIGINAL workspace.
      await page.reload();
      await openPicker(page, ws.id);
      await expect(page.locator('[data-testid="picker-mode-orchestration"]')).toHaveAttribute("data-selected", "true");
      await expect(dropdown(page, "picker-model-planner")).toHaveAttribute("data-value", changedModel);
      await expect(dropdown(page, "picker-workspace")).toHaveAttribute("data-value", ws.id);
    } finally {
      await cleanupWorkspace(api, otherWs.id);
    }
  });

  test("@golden IN-PLACE PAIR SPAWN: confirm materializes orchestrator + planner, no husk left behind", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    // Fresh workspace: no backend tabs before confirm (picker is virtual).
    const before = await muxState(api);
    expect(visibleTabsInWorkspace(before, ws.id)).toHaveLength(0);

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    // The picker retires; the group (orchestrator + planner child) appears.
    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);

    const { tab: orchestrator } = await waitForOrchestrator(api, ws.id);
    expect(orchestrator.role).toBe("Orchestrator");
    // No leftover virtual husk anywhere in the snapshot.
    const after = await muxState(api);
    expect(Object.values(after.tabs).some((p) => p.title === "New session")).toBe(false);

    // is never the bare, pile-up-prone "Orchestration".
    // Plan creation is the live orchestrator's first act (lazy, LLM-driven) —
    // covered in picker-live.spec.ts; the stub transport verifies pair plumbing.
});

  test("@golden GREETING: orchestrator's first assistant message is the PEER_ORCH_GREETING, unprompted", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    const { tab } = await waitForOrchestrator(api, ws.id);
    const segments = peerOrchGreetingSegments();
    await expect
      .poll(
        async () => {
          try {
            const res = await api.post<{ messages?: { type?: string; content?: string }[] }>(
              `/tabs/${tab.id}/chat-raw/start`,
            );
            return res.messages?.[0]?.content ?? "";
          } catch {
            return "";
          }
        },
        { timeout: 60_000 },
      )
      // the subordinate-framing reword tracks automatically.
      .toContain(segments[0]);
    const greeting = (
      await api.post<{ messages?: { content?: string }[] }>(`/tabs/${tab.id}/chat-raw/start`)
    ).messages?.[0]?.content ?? "";
    for (const seg of segments) {
      expect(greeting, `greeting must contain constant segment "${seg}"`).toContain(seg);
    }
  });

  test("@golden CROSS-WORKSPACE: spawning into workspace B leaves NO husk in A", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    const wsB = await createTestWorkspace(api, "pw-picker-cross-b");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      await selectDropdown(page, "picker-workspace", wsB.id);
      await page.locator('[data-testid="picker-confirm"]').click();

      await expect
        .poll(async () => visibleTabsInWorkspace(await muxState(api), wsB.id).length, {
          timeout: 60_000,
        })
        .toBeGreaterThanOrEqual(2);
      const { tab: orchestrator } = await waitForOrchestrator(api, wsB.id);
      expect(orchestrator.role, "orchestrator must land in workspace B").toBe("Orchestrator");

      // …and A keeps NO husk: the virtual picker just vanished, no backend tab.
      const after = await muxState(api);
      expect(visibleTabsInWorkspace(after, ws.id)).toHaveLength(0);
    } finally {
      await cleanupWorkspace(api, wsB.id);
    }
  });

  test("@golden SINGLE SELECT: Start closes the picker and selects + focuses the created agent tab", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-single"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();
    // Single Claude asks about skip-permissions first — launch through it.
    const launch = page.getByRole("button", { name: "Launch" });
    if (await launch.isVisible({ timeout: 3_000 }).catch(() => false)) await launch.click();

    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    const { tab, tab } = await waitForSingleAgent(api, ws.id);
    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.active_tab_id ?? null, {
        timeout: 30_000,
      })
      .toBe(tab.id);
    const s = await muxState(api);
    expect(s.active_workspace_id, "active workspace is the picker host").toBe(ws.id);
    expect(s.focused_tab_id, "the created agent tab is focused").toBe(tab.id);
    // No lingering virtual picker tab in the workspace.
    expect(s.workspaces.find((w) => w.id === ws.id)?.tabs.every((t) => t.id !== "virtual"), "no virtual tab").toBe(true);
  });

  test("@golden PAIR SELECT: Start selects + focuses the ORCHESTRATOR tab of the new group", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
    const { tab: orchTab, tab: orchestrator } = await waitForOrchestrator(api, ws.id);
    await expect
      .poll(async () => (await muxState(api)).workspaces.find((w) => w.id === ws.id)?.active_tab_id ?? null, {
        timeout: 30_000,
      })
      .toBe(orchTab.id);
    const s = await muxState(api);
    expect(s.active_workspace_id, "active workspace is the picker host").toBe(ws.id);
    // Stub agents boot in seconds, so the planner's focus grab can land after
    // the product refocuses the orchestrator — assert the settled state.
    await expect
      .poll(async () => (await muxState(api)).focused_tab_id, { timeout: 30_000 })
      .toBe(orchestrator.id);

    await ui.page.screenshot({ path: "tests/playwright/artifacts/picker-pair-selected.png" });
  });

  test("@golden CROSS-WORKSPACE SELECT: Start switches the active workspace to B and selects its orchestrator", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    const wsB = await createTestWorkspace(api, "pw-picker-cross-sel-b");
    try {
      await page.locator('[data-testid="picker-mode-orchestration"]').click();
      await selectDropdown(page, "picker-workspace", wsB.id);
      await page.locator('[data-testid="picker-confirm"]').click();

      await expect(page.locator('[data-testid="new-session-picker"]')).toBeHidden({ timeout: 60_000 });
      const { tab: orchTab, tab: orchestrator } = await waitForOrchestrator(api, wsB.id);
      await expect
        .poll(async () => (await muxState(api)).active_workspace_id ?? null, { timeout: 30_000 })
        .toBe(wsB.id);
      const s = await muxState(api);
      expect(s.workspaces.find((w) => w.id === wsB.id)?.active_tab_id, "B's orchestrator tab is active").toBe(orchTab.id);
      await expect
      .poll(async () => (await muxState(api)).focused_tab_id, { timeout: 30_000 })
      .toBe(orchestrator.id);
      // Host A keeps no husk.
      expect(visibleTabsInWorkspace(s, ws.id)).toHaveLength(0);
    } finally {
      await cleanupWorkspace(api, wsB.id);
    }
  });

  test("@golden GROUP CLOSE: manager row closes individually; head close confirms when group has members", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await page.locator('[data-testid="picker-confirm"]').click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);

    const { tab: orchTab } = await waitForOrchestrator(api, ws.id);
    const state = await muxState(api);
    const plannerTab = visibleTabsInWorkspace(state, ws.id).find((t) => t.id !== orchTab.id)!;

    const plannerRow = page.locator(`[data-tab-id="${plannerTab.id}"]`);
    await plannerRow.hover();
    await plannerRow.locator('button[title="Tab actions"]').click();
    const plannerClose = page.locator('[data-testid="tab-menu-close"]');
    await expect(plannerClose).toBeVisible({ timeout: 5_000 });
    await plannerClose.click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(1);

    const headRow = page.locator(`[data-tab-id="${orchTab.id}"]`);
    await expect(headRow).toBeVisible({ timeout: 15_000 });
    await headRow.hover();
    await headRow.locator('button[title="Tab actions"]').click();
    const headClose = page.locator('[data-testid="tab-menu-close"]');
    await expect(headClose).toBeVisible({ timeout: 5_000 });
    await headClose.click();

    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(0);
  });

  test("@golden SHARED DROPDOWN + VIRTUAL PHYSICALITY (scenario 11)", async ({ api, ui }) => {
    const page = ui.page;
    const picker = page.locator('[data-testid="new-session-picker"]');

    await page.locator('[data-testid="picker-mode-orchestration"]').click();
    await expect(picker.locator("select")).toHaveCount(0);
    const provider = dropdown(page, "picker-provider-orchestrator");
    await expect(provider).toBeVisible();
    await provider.click();
    // queryable DOM nodes, which a native <select> popup never is.
    await expect(page.locator('[data-testid="picker-provider-orchestrator-option-claude"]')).toBeVisible({ timeout: 5_000 });
    await page.keyboard.press("Escape");
    await expect(picker, "picker stayed mounted — no reload/flicker").toBeVisible();

    await page.reload();
    await expect(picker).toBeHidden();
    const baseTabs = await api.tabs();
    const baseState = await muxState(api);
    const baseTabs = visibleTabsInWorkspace(baseState, ws.id).length;

    await openPicker(page, ws.id);

    const openTabs = await api.tabs();
    expect(tabCount(openTabs), "tab count unchanged while picker open").toBe(tabCount(baseTabs));
    expect(ptyCount(openTabs), "PTY count unchanged while picker open").toBe(ptyCount(baseTabs));
    const openState = await muxState(api);
    expect(
      visibleTabsInWorkspace(openState, ws.id).length,
      "no rendered backend tab added on open",
    ).toBe(baseTabs);
    expect(
      Object.values(openState.tabs).some((p) => p.title === "New session"),
      "snapshot has no picker tab entry",
    ).toBe(false);

    // Ghost-title guard at picker-open: no sidebar tab renders empty/"…".
    for (const title of await sidebarTabTitles(page, ws.id)) {
      expect(isPlaceholderTitle(title), `open-state tab title "${title}" must be titled`).toBe(false);
    }
  });

  test("@golden NO GHOST TITLE across a pair spawn (scenario 12)", async ({ api, ui }) => {
    test.setTimeout(120_000);
    await requireStub(api);
    const page = ui.page;
    await page.locator('[data-testid="picker-mode-orchestration"]').click();

    // any empty/"…" placeholder frame the moment it appears.
    let ghost: string | null = null;
    let polling = true;
    const poller = (async () => {
      while (polling) {
        for (const title of await sidebarTabTitles(page, ws.id).catch(() => [])) {
          if (isPlaceholderTitle(title)) ghost = title;
        }
        await new Promise((r) => setTimeout(r, 60));
      }
    })();

    await page.locator('[data-testid="picker-confirm"]').click();
    await waitForOrchestrator(api, ws.id);
    // Let the group settle (planner child materializes + titles resolve).
    await expect
      .poll(async () => visibleTabsInWorkspace(await muxState(api), ws.id).length, {
        timeout: 60_000,
      })
      .toBe(2);
    polling = false;
    await poller;

    // Settled snapshot: every rendered tab is titled.
    for (const title of await sidebarTabTitles(page, ws.id)) {
      expect(isPlaceholderTitle(title), `settled tab title "${title}" must be titled`).toBe(false);
    }
    expect(ghost, `a placeholder tab title "${ghost}" appeared mid-spawn`).toBeNull();
  });
});

/**
 * Ghost-tab invariant (task_63c585953fef4dbdb9e1d906077f8b18)
 *
 * These tests verify that no sidebar tab ever renders as a bare "…"/empty
 * placeholder — whether from snapshot restore, a leaked virtual tab, or a
 * blank backend tab that lost its title. The Rust unit tests in
 * snapshot.rs prove the root fix (skip_snapshot); these Playwright tests
 * guard the invariant at the UI layer on a live dev session.
 */
test.describe("ghost-tab invariant", () => {
  test.describe.configure({ timeout: 30_000, retries: 0 });

  test("@golden GHOST-TAB: current session sidebar has no placeholder titles", async ({ api, ui }) => {
    // tab restored with a blank title, this catches it immediately.
    const state = await muxState(api);
    const allTitles: Array<{ ws: string; title: string }> = [];

    for (const w of state.workspaces) {
      for (const title of await sidebarTabTitles(ui.page, w.id).catch(() => [])) {
        allTitles.push({ ws: w.id, title });
      }
    }

    for (const { ws, title } of allTitles) {
      expect(
        isPlaceholderTitle(title),
        `workspace "${ws}" sidebar tab "${title}" is a ghost placeholder`,
      ).toBe(false);
    }
  });

  test("@golden GHOST-TAB: skip_snapshot tab is absent from backend snapshot after picker creates it", async ({ api, ui }) => {
    const testWs = await createTestWorkspace(api, `${TEST_WS_PREFIX}-ghost-snap`);
    try {
      await openPicker(ui.page, testWs.id);

      // Virtual picker open: backend must have ZERO rendered tabs in this workspace.
      const liveState = await muxState(api);
      expect(
        visibleTabsInWorkspace(liveState, testWs.id),
        "virtual picker must not create a rendered backend tab (would restore as ghost on restart)",
      ).toHaveLength(0);

      // Every visible sidebar title is real, not a placeholder.
      for (const title of await sidebarTabTitles(ui.page, testWs.id).catch(() => [])) {
        expect(isPlaceholderTitle(title), `sidebar tab "${title}" is a ghost placeholder`).toBe(false);
      }
    } finally {
      await api.closeWorkspace(testWs.id).catch(() => {});
    }
  });
});
