/**
 * WHAT: mobile overflow covers broad web surfaces at phone widths.
 * WHY: 320px regressions have repeatedly come from fixed-width panels, menus,
 * and dialogs rather than from the terminal/code scroll surfaces themselves.
 * ORACLE: visible surfaces and controls are fully inside the viewport and
 * reachable at their center; document scroll width is only a secondary signal.
 */
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { expect, test, type Browser, type Page, type SpawnAgent, type TabCleanup, API_BASE, readDevToken } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";
import { auditKnownSurfaces, collectViewportOffenders, INTERACTIVE, revealOwnedHorizontalSurface, type ReachabilityMode } from "../../helpers/mobile-overflow";
import { tempSpawnCwd } from "../../helpers/spawn-cwd";
import { isStubTransportActive } from "../../helpers/stub-gate";

const VIEWPORTS = [
  { width: 320, height: 568, name: "320x568" },
  { width: 390, height: 844, name: "390x844" },
  { width: 430, height: 932, name: "430x932" },
] as const;

type CanvasLayerExpectation = "overlay" | "inline";

type TargetTab = {
  tabId: string;
  tabTitle: string;
  workspaceId: string;
  planId?: string;
  teamId?: string;
  fixtureWorkspaceId?: string;
};
type MuxState = {
  workspaces: Array<{
    id: string;
    cwd?: string | null;
    /** `Workspace.tabs` is a list of tab ids (src/lib/types.ts `tabs: string[]`),
     *  never objects carrying `tab_id`. */
    tabs: string[];
  }>;
};
type Health = {
  build_sha?: string;
  build?: string;
  instance?: { identifier?: string };
};

async function newWebPage(browser: Browser, viewport: { width: number; height: number }): Promise<Page> {
  const ctx = await browser.newContext({ viewport });
  const page = await ctx.newPage();
  await page.emulateMedia({ reducedMotion: "reduce" });
  return page;
}

async function openApp(page: Page): Promise<void> {
  await page.goto(`${API_BASE}/#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"], [data-testid="web-sidebar-open"]', { timeout: 20_000 });
}

async function openSidebar(page: Page, workspaceName?: string): Promise<void> {
  const burger = page.locator('[data-testid="web-sidebar-open"]');
  if (await burger.isVisible().catch(() => false)) {
    await burger.click();
  }
  await expect(page.locator(".ct-sidebar-drawer.is-open"), "drawer-never-opened").toBeAttached({ timeout: 5_000 });
  const workspace = workspaceName
    ? page.getByRole("region", { name: `${workspaceName} workspace contents` }).or(
      page.getByRole("button", { name: `Select workspace ${workspaceName}` }),
    ).first()
    : page.getByRole("region", { name: / workspace contents$/ }).or(
      page.getByRole("button", { name: /Select workspace / }),
    ).or(page.getByRole("button", { name: "Create workspace" })).first();
  await workspace.waitFor({ state: "visible", timeout: 10_000 });
  await settleLayout(page);
}

async function closeSidebar(page: Page): Promise<void> {
  const backdrop = page.locator(".ct-sidebar-backdrop").first();
  if (await backdrop.isVisible().catch(() => false)) {
    await backdrop.click({ position: { x: 10, y: 10 } });
  }
  await page.waitForFunction(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer");
    const backdrop = document.querySelector(".ct-sidebar-backdrop");
    if (!drawer) return true;
    const drawerStyle = window.getComputedStyle(drawer);
    const backdropStyle = backdrop ? window.getComputedStyle(backdrop) : null;
    return !drawer.classList.contains("is-open") &&
      drawer.getAttribute("aria-hidden") === "true" &&
      drawer.hasAttribute("inert") &&
      drawerStyle.pointerEvents === "none" &&
      (!backdrop || (
        backdrop.getAttribute("aria-hidden") === "true" &&
        backdropStyle?.pointerEvents === "none"
      ));
  }, null, { timeout: 5_000 });
  await settleLayout(page);
}

async function settleLayout(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  }));
}

async function assertHealthMatchesHead(): Promise<Health> {
  const expected = process.env.CT_E2E_EXPECTED_BUILD_SHA ?? execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const health = await fetch(`${API_BASE}/api/health`, { signal: AbortSignal.timeout(5_000) }).then((r) => r.json()) as Health;
  expect(health.build_sha, `health build_sha ${JSON.stringify(health)}`).toBe(expected);
  return health;
}

async function assertBuildIdentity(page: Page, health: Health): Promise<void> {
  const badge = page.getByLabel(/Development build, version /);
  const isDev = Boolean(health.instance?.identifier?.trim());
  await expect(badge).toHaveCount(isDev ? 1 : 0);
}

async function auditViewport(page: Page, label: string, options: { reachability?: ReachabilityMode } = {}): Promise<void> {
  const doc = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
    bodyScrollWidth: document.body.scrollWidth,
    bodyClientWidth: document.body.clientWidth,
  }));
  expect(
    doc.scrollWidth,
    `${label} secondary document overflow signal ${JSON.stringify(doc)}`,
  ).toBeLessThanOrEqual(doc.clientWidth + 1);

  if (options.reachability === "open-drawer") {
    await waitForOpenDrawerStable(page);
  } else if (options.reachability === "settings-menu") {
    await page.locator('[data-testid="settings-menu"]').waitFor({ state: "visible", timeout: 5_000 });
  } else if (options.reachability === "workspace-dropdown") {
    await page.locator('[data-testid^="picker-workspace-option-"]').first().waitFor({ state: "visible", timeout: 5_000 });
  }
  const offenders = await collectViewportOffenders(page, options.reachability ?? "all");
  expect(offenders, `${label} offscreen or unreachable controls`).toEqual([]);
  if (options.reachability === "open-drawer") {
    await assertOpenDrawerControlsReachable(page, label);
    await assertSidebarScrimReachable(page, label);
  }
}

/**
 * A role is only assignable to a tab whose agent already has a VERIFIED live
 * session: both `make-orchestrator` and `POST /tabs/{id}/role` transition with
 * `require_verified_session: true`, and `agent_transition.rs` then demands an
 * `agent_session_id` plus a session-binding nonce the agent has REPORTED back.
 * `POST /tabs/{id}/agent-type` only declares a type — it spawns nothing — so a
 * bare tab can never be promoted. `spawn_agent` is the one path that does not
 * require a prior session, because it establishes one; a Team anchor therefore
 * has to be spawned with its role, not created bare and promoted afterwards.
 */
async function prepareTargetTab(
  api: ApiClient,
  tabCleanup: TabCleanup,
  seedTeam: boolean,
  spawnAgent: SpawnAgent,
): Promise<TargetTab> {
  const tabTitle = `pw-mobile-layer-${Date.now()}`;
  let fixtureWorkspaceId: string | undefined;
  let fixtureCwd: string | undefined;
  if (seedTeam) {
    fixtureCwd = tempSpawnCwd("pw-mobile-team-");
    await api.createWorkspace(`pw mobile Team ${Date.now()}`, fixtureCwd);
    fixtureWorkspaceId = (await api.workspaces()).find((workspace) => workspace.cwd === fixtureCwd)?.id;
    if (!fixtureWorkspaceId) throw new Error(`project-backed Team workspace was not created for ${fixtureCwd}`);
  }

  let tabId: string;
  if (seedTeam) {
    const spawned = await spawnAgent({
      agent: "claude",
      role: "Orchestrator",
      title: tabTitle,
      workspace_id: fixtureWorkspaceId,
      cwd: fixtureCwd,
      skip_permissions: true,
      activate: true,
    });
    tabId = spawned.tab_id;
    tabCleanup.track(tabId);
  } else {
    const spawned = await spawnAgent({
      agent: "claude",
      title: tabTitle,
      skip_permissions: true,
      activate: true,
    });
    tabId = spawned.tab_id;
  }
  await api.post(`/tabs/${tabId}/chat-view`, { enabled: true });
  await api.focusTab(tabId);

  const mux = await api.get<MuxState>("/mux/state");
  for (const workspace of mux.workspaces) {
    if (fixtureWorkspaceId && workspace.id !== fixtureWorkspaceId) continue;
    if (!workspace.tabs.includes(tabId)) continue;
    if (!seedTeam) return { tabId, tabTitle, workspaceId: workspace.id };
    if (!workspace.cwd) throw new Error(`Team fixture workspace ${workspace.id} has no cwd`);
    const plan = await api.post<{ plan_id: string; team_id: string }>("/plans", {
      title: `pw-mobile-team-${Date.now()}`,
      project_path: workspace.cwd,
      owner: { tab_id: tabId, session_id: null, title: tabTitle },
      kinds: "implement:#38bdf8",
    });
    return {
      tabId,
      tabTitle,
      workspaceId: workspace.id,
      planId: plan.plan_id,
      teamId: plan.team_id,
      fixtureWorkspaceId,
    };
  }
  throw new Error(`created tab ${tabId} has no owning workspace in mux state`);
}

async function assertTargetChatReady(
  page: Page,
  target: Pick<TargetTab, "tabId" | "tabTitle">,
  label: string,
): Promise<void> {
  await expect(
    page.getByRole("region", { name: `Tab chat: ${target.tabTitle}` }),
    `${label} authenticated tab chat ready`,
  ).toBeVisible({ timeout: 10_000 });
  await expect(
    page.locator(`[data-tab-id="${target.tabId}"]`).getByText("Loading chat…", { exact: true }),
    `${label} chat must not remain loading`,
  ).toHaveCount(0);
  await expect(
    page.locator(`[data-testid="tab-session-chat"][data-tab-id="${target.tabId}"]`),
    `${label} target tab chat visible before canvas`,
  ).toBeVisible({ timeout: 10_000 });
}

async function waitSidebarClosed(page: Page): Promise<void> {
  await page.waitForFunction(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer");
    const backdrop = document.querySelector(".ct-sidebar-backdrop");
    if (!drawer) return true;
    const drawerStyle = window.getComputedStyle(drawer);
    const backdropStyle = backdrop ? window.getComputedStyle(backdrop) : null;
    return !drawer.classList.contains("is-open") &&
      drawer.getAttribute("aria-hidden") === "true" &&
      drawer.hasAttribute("inert") &&
      drawerStyle.pointerEvents === "none" &&
      (!backdrop || (
        backdrop.getAttribute("aria-hidden") === "true" &&
        backdropStyle?.pointerEvents === "none"
      ));
  }, null, { timeout: 5_000 });
  await settleLayout(page);
}

async function openSidebarAndSelectTargetTab(api: ApiClient, page: Page, target: TargetTab, label: string): Promise<void> {
  await api.focusWorkspace(target.workspaceId);
  const workspace = (await api.workspaces()).find((candidate) => candidate.id === target.workspaceId);
  if (!workspace) throw new Error(`${label} target workspace ${target.workspaceId} is missing`);
  await openSidebar(page, workspace.name);

  const workspaceRegion = page.getByRole("region", { name: `${workspace.name} workspace contents` });
  if (!workspace.protected) {
    const workspaceButton = page.getByRole("button", { name: `Select workspace ${workspace.name}` }).first();
    await expect(workspaceButton, `${label} target workspace visible`).toBeVisible({ timeout: 10_000 });
    if ((await workspaceButton.getAttribute("aria-pressed")) !== "true") await workspaceButton.click();
  }
  await expect(workspaceRegion, `${label} selected workspace region visible`).toBeVisible({ timeout: 10_000 });
  const tab = workspaceRegion.getByRole("button", { name: `Open tab ${target.tabTitle}` }).first();
  if (workspace.protected && !(await tab.isVisible().catch(() => false))) {
    const disclosure = workspaceRegion.getByRole("button", { name: workspace.name }).first();
    if ((await disclosure.getAttribute("aria-expanded")) !== "true") await disclosure.click();
  }
  await expect(tab, `${label} target tab tab visible`).toBeVisible({ timeout: 10_000 });
  await tab.click();
  await waitSidebarClosed(page);
}

async function openWorkspaceCanvasOverlay(page: Page, label: string): Promise<void> {
  const button = page.getByRole("button", { name: "Open workspace canvas" }).first();
  await expect(button, `${label} canvas toolbar button visible`).toBeVisible({ timeout: 10_000 });
  await button.click();
  await expect(
    page.locator('[data-testid="canvas-view"], [data-testid="plan-view-shell"], [data-testid="canvas-empty"]').first(),
    `${label} workspace canvas visible`,
  ).toBeVisible({ timeout: 10_000 });
  await settleLayout(page);
}

async function assertActiveManagerReadable(
  api: ApiClient,
  page: Page,
  target: TargetTab,
  label: string,
): Promise<void> {
  if (!target.teamId) throw new Error(`${label} has no seeded Team`);
  const team = page.locator(`[data-testid="team-unit"][data-team-id="${target.teamId}"]`);
  await expect(team, `${label} seeded Team visible`).toBeVisible({ timeout: 10_000 });
  const managers = team.getByTestId("managers-section");
  await expect(managers, `${label} managers section visible`).toBeVisible({ timeout: 10_000 });
  const manager = managers.getByRole("button", {
    name: `Open tab ${target.tabTitle}`,
    exact: true,
  });
  const accessibleName = await manager.getAttribute("aria-label");
  expect(accessibleName, `${label} manager accessible name`).toMatch(/^Open tab \S/);
  await manager.click();
  await waitSidebarClosed(page);

  await openSidebar(page);
  const activeTeam = page.locator(`[data-testid="team-unit"][data-team-id="${target.teamId}"]`);
  const activeManager = activeTeam.getByRole("button", { name: accessibleName!, exact: true });
  await expect(activeManager, `${label} active manager button visible`).toBeVisible({ timeout: 10_000 });
  const row = activeManager.locator("xpath=..");
  await expect(row.locator(":scope > .active-tab-pill"), `${label} active fill behind manager`).toHaveCount(1);
  await expect(activeManager.locator('[data-testid="tab-title"]'), `${label} active manager title visible`)
    .toHaveText(/\S+/);

  await openSidebarAndSelectTargetTab(api, page, target, `${label} restore target`);
}

async function assertCanvasStateBeforeTabTransition(
  page: Page,
  tabId: string,
  expected: CanvasLayerExpectation,
  label: string,
): Promise<void> {
  const state = await page.evaluate(({ targetTabId, expectedLayer }) => {
    const canvas = document.querySelector('[data-testid="canvas-view"], [data-testid="plan-view-shell"], [data-testid="canvas-empty"]') as HTMLElement | null;
    if (!canvas) return { kind: "missing-canvas" as const };
    const visible = (el: Element | null): el is HTMLElement => {
      if (!(el instanceof HTMLElement)) return false;
      const rect = el.getBoundingClientRect();
      const style = window.getComputedStyle(el);
      return style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
    };
    const visibleControlsOutsideCanvas = Array.from(document.querySelectorAll(
      '[data-testid="tab-session-chat"], [data-testid="terminal-tab"], [data-testid="chat-composer"], [data-testid="chat-reconnect"]',
    )).filter((el) => visible(el) && !el.closest('[inert], [aria-hidden="true"]') && !canvas.contains(el));
    if (expectedLayer === "inline") {
      return {
        kind: "inline" as const,
        tabControlCount: visibleControlsOutsideCanvas.length,
      };
    }
    const tabRoot = document.querySelector(`[data-tab-id="${targetTabId}"]`) as HTMLElement | null;
    if (!visible(tabRoot)) {
      return {
        kind: "overlay" as const,
        tabRootVisible: false,
      };
    }
    const rect = tabRoot.getBoundingClientRect();
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
    const hit = document.elementFromPoint(x, y);
    return {
      kind: "overlay" as const,
      tabRootVisible: true,
      hitInsideTabRoot: hit instanceof Node && (hit === tabRoot || tabRoot.contains(hit)),
      hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
    };
  }, { targetTabId: tabId, expectedLayer: expected });

  expect(state.kind, `${label} valid canvas state before tab transition ${JSON.stringify(state)}`).not.toBe("missing-canvas");
  if (state.kind === "inline") {
    expect(state.tabControlCount, `${label} inline canvas has no outside-canvas chat/terminal controls`).toBe(0);
    return;
  }
  expect(state.tabRootVisible, `${label} overlay target tab root visible ${JSON.stringify(state)}`).toBe(true);
  expect(state.hitInsideTabRoot, `${label} overlay canvas blocks click-through to tab root ${JSON.stringify(state)}`).toBe(false);
}

async function assertWorkspaceLayerTopmost(page: Page, tabId: string, label: string): Promise<void> {
  await expect
    .poll(
      () => page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`).evaluate((chat) => {
        const rect = chat.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) return false;
        const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
        const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
        const hit = document.elementFromPoint(x, y);
        return hit instanceof Node && (hit === chat || chat.contains(hit));
      }).catch(() => false),
      { timeout: 5_000, message: `${label} workspace layer never became topmost` },
    )
    .toBe(true);

  const result = await page.locator(`[data-testid="tab-session-chat"][data-tab-id="${tabId}"]`).evaluate((chat) => {
    const rect = chat.getBoundingClientRect();
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
    const hit = document.elementFromPoint(x, y);
    return {
      hitInsideChat: hit instanceof Node && (hit === chat || chat.contains(hit)),
      hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
    };
  });
  expect(result.hitInsideChat, `${label} workspace layer never became topmost ${JSON.stringify(result)}`).toBe(true);
}

async function assertPeerPickerTopContained(page: Page, label: string): Promise<void> {
  const box = await page.locator('[data-testid="peer-picker-button"]').boundingBox();
  expect(box, `${label} peer picker button box`).not.toBeNull();
  expect(box!.y, `${label} peer picker button top`).toBeGreaterThanOrEqual(-1);
}

async function assertPeerPickerReachable(page: Page, label: string): Promise<void> {
  const result = await page.locator('[data-testid="peer-picker-button"]').evaluate((el) => {
    const rect = el.getBoundingClientRect();
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
    const hit = document.elementFromPoint(x, y);
    return {
      reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
      hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
      hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
    };
  });
  expect(result.reachable, `${label} peer picker reachable ${JSON.stringify(result)}`).toBe(true);
}

async function dismissPeerPickerWithSupportedControl(page: Page, label: string): Promise<void> {
  await page.locator('[data-testid="peer-picker-button"]').click();
  await page.locator('[data-testid="peer-picker-menu"]').waitFor({ state: "hidden", timeout: 5_000 });
  await settleLayout(page);
  await assertPeerPickerReachable(page, `${label} after peer picker dismiss`);
}

async function dismissWorkspaceDropdownWithSupportedControl(page: Page, label: string): Promise<void> {
  await page.locator('[data-testid="picker-workspace"]').first().click();
  await page.waitForFunction(() => {
    const option = document.querySelector('[data-testid^="picker-workspace-option-"]') as HTMLElement | null;
    const root = option?.closest(".ct-float") as HTMLElement | null;
    const visible = (el: HTMLElement | null) => !!el && window.getComputedStyle(el).display !== "none" && window.getComputedStyle(el).visibility !== "hidden" && el.getBoundingClientRect().width > 0;
    return !visible(option) && !visible(root);
  }, null, { timeout: 5_000 });
  await settleLayout(page);
  await assertControlsReachable(page, '[data-testid^="picker-provider-"], [data-testid^="picker-model-"]', `${label} after workspace dismiss`);
}

async function assertControlsReachable(page: Page, selector: string, label: string): Promise<void> {
  const results = await page.locator(selector).evaluateAll((nodes) => nodes.map((el) => {
    const rect = (el as HTMLElement).getBoundingClientRect();
    const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el));
  }));
  expect(results.length, `${label} controls present`).toBeGreaterThan(0);
  expect(results.every(Boolean), `${label} controls reachable`).toBe(true);
}

function targetTabModeSwitch(page: Page, tabId: string) {
  return page.locator(`[data-tab-id="${tabId}"]`).locator('[data-testid="tab-mode-switch"]');
}

async function assertTabModeTabsReachable(page: Page, tabId: string, label: string): Promise<void> {
  const switchLocator = targetTabModeSwitch(page, tabId);
  const switchCount = await switchLocator.count();
  expect(switchCount, `${label} target tab mode switch count for ${tabId}`).toBe(1);

  const results = await switchLocator.locator('[role="tab"]').evaluateAll((nodes) =>
    nodes.map((node) => {
      const el = node as HTMLElement;
      const rect = el.getBoundingClientRect();
      const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
      const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
      const hit = document.elementFromPoint(x, y);
      return {
        label: (el.innerText || el.textContent || "").trim(),
        reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
        hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      };
    }));
  const tabNames = results.map((item) => item.label).sort();
  expect(tabNames, `${label} tab mode tabs present ${JSON.stringify(results)}`).toEqual(["Chat", "Terminal"]);
  expect(
    results.filter((item) => item.label === "Chat"),
    `${label} one Chat control ${JSON.stringify(results)}`,
  ).toHaveLength(1);
  expect(
    results.filter((item) => item.label === "Terminal"),
    `${label} one Terminal control ${JSON.stringify(results)}`,
  ).toHaveLength(1);
  expect(results.every((item) => item.reachable), `${label} tab mode tabs reachable ${JSON.stringify(results)}`).toBe(true);
}

async function assertPeerPickerMenuInverseOcclusion(page: Page, tabId: string, label: string): Promise<void> {
  const switchLocator = targetTabModeSwitch(page, tabId);
  const switchCount = await switchLocator.count();
  expect(switchCount, `${label} inverse-occlusion target tab mode switch count for ${tabId}`).toBe(1);

  const result = await switchLocator.locator('[role="tab"]').evaluateAll((nodes) => {
    const menu = document.querySelector('[data-testid="peer-picker-menu"]');
    return nodes.map((node) => {
      const el = node as HTMLElement;
      const rect = el.getBoundingClientRect();
      const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
      const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
      const hit = document.elementFromPoint(x, y);
      return {
        label: (el.innerText || el.textContent || "").trim(),
        hitInPeerPickerMenu: hit instanceof Node && !!menu && (hit === menu || menu.contains(hit)),
        reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
        hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      };
    });
  });
  const tabNames = result.map((item) => item.label).sort();
  expect(tabNames, `${label} inverse-occlusion tab mode tabs present ${JSON.stringify(result)}`).toEqual(["Chat", "Terminal"]);
  expect(
    result.filter((item) => item.label === "Chat"),
    `${label} inverse-occlusion one Chat control ${JSON.stringify(result)}`,
  ).toHaveLength(1);
  expect(
    result.filter((item) => item.label === "Terminal"),
    `${label} inverse-occlusion one Terminal control ${JSON.stringify(result)}`,
  ).toHaveLength(1);
  expect(
    result.every((item) => item.hitInPeerPickerMenu && !item.reachable),
    `${label} inverse-occlusion peer menu covers Chat and Terminal tabs ${JSON.stringify(result)}`,
  ).toBe(true);
}

async function waitForOpenDrawerStable(page: Page): Promise<void> {
  await page.evaluate(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer.is-open");
    if (!drawer) return Promise.resolve();
    const finiteAnimations = drawer
      .getAnimations()
      .filter((animation) =>
        animation.effect instanceof KeyframeEffect &&
        animation.effect.target === drawer &&
        Number.isFinite(animation.effect.getTiming().duration as number)
      );
    return Promise.race([
      Promise.all(finiteAnimations.map((animation) => animation.finished.catch(() => undefined))),
      new Promise((resolve) => setTimeout(resolve, 1_500)),
    ]);
  });
  await settleLayout(page);
}

async function assertOpenDrawerControlsReachable(page: Page, label: string): Promise<void> {
  const offenders = await page.locator(".ct-sidebar-drawer.is-open").locator(INTERACTIVE).evaluateAll((nodes) => {
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const clips = (value: string) => /auto|scroll|hidden|clip/.test(value);
    const isFullyClippedByAncestor = (el: HTMLElement, rect: DOMRect) => {
      let parent = el.parentElement;
      while (parent && parent !== document.body && parent !== document.documentElement) {
        const style = window.getComputedStyle(parent);
        const clipsX = clips(style.overflowX);
        const clipsY = clips(style.overflowY);
        if (clipsX || clipsY) {
          const clipRect = parent.getBoundingClientRect();
          const clippedX = rect.right <= clipRect.left || rect.left >= clipRect.right;
          const clippedY = rect.bottom <= clipRect.top || rect.top >= clipRect.bottom;
          if ((clipsX && clippedX) || (clipsY && clippedY)) return true;
        }
        parent = parent.parentElement;
      }
      return false;
    };
    return nodes
      .map((node) => {
        const el = node as HTMLElement;
        el.scrollIntoView({ block: "nearest", inline: "nearest" });
        const style = window.getComputedStyle(el);
        const rect = el.getBoundingClientRect();
        const hidden =
          style.visibility === "hidden" ||
          style.display === "none" ||
          rect.width <= 0 ||
          rect.height <= 0 ||
          isFullyClippedByAncestor(el, rect);
        if (hidden) return null;
        const left = rect.left;
        const right = rect.right;
        const top = rect.top;
        const bottom = rect.bottom;
        const inBounds =
          left >= -1 &&
          right <= viewportWidth + 1 &&
          top >= -1 &&
          bottom <= viewportHeight + 1;
        if (!inBounds) return null;
        const centerX = Math.min(Math.max(left + rect.width / 2, 0), viewportWidth - 1);
        const centerY = Math.min(Math.max(top + rect.height / 2, 0), viewportHeight - 1);
        const hit = document.elementFromPoint(centerX, centerY);
        const reachable = hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el));
        if (reachable) return null;
        return {
          tag: el.tagName.toLowerCase(),
          testid: el.getAttribute("data-testid"),
          text: (el.innerText || el.textContent || "").trim().slice(0, 80),
          left,
          top,
          right,
          bottom,
          hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
          hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
        };
      })
      .filter(Boolean);
  });
  expect(offenders, `${label} unreachable drawer controls`).toEqual([]);
}

async function assertSidebarScrimReachable(page: Page, label: string): Promise<void> {
  const scrim = await page.evaluate(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer.is-open");
    const backdrop = document.querySelector(".ct-sidebar-backdrop.is-open");
    if (!drawer || !backdrop) return { ok: false, reason: "missing-layer" };
    const drawerRect = drawer.getBoundingClientRect();
    const backdropRect = backdrop.getBoundingClientRect();
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const x = Math.min(viewportWidth - 8, Math.max(drawerRect.right + 8, 8));
    const y = Math.min(viewportHeight - 8, Math.max(drawerRect.top + 24, 8));
    const hit = document.elementFromPoint(x, y);
    return {
      coversViewport:
        backdropRect.left <= 1 &&
        backdropRect.top <= 1 &&
        backdropRect.right >= viewportWidth - 1 &&
        backdropRect.bottom >= viewportHeight - 1,
      hitOk: hit === backdrop || (hit instanceof Node && backdrop.contains(hit)),
      x,
      y,
      backdropLeft: backdropRect.left,
      backdropTop: backdropRect.top,
      backdropRight: backdropRect.right,
      backdropBottom: backdropRect.bottom,
      viewportWidth,
      viewportHeight,
      hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
      hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      hitClass: hit instanceof HTMLElement ? hit.className : null,
    };
  });
  expect(scrim.coversViewport, `${label} sidebar scrim viewport coverage ${JSON.stringify(scrim)}`).toBe(true);
  expect(scrim.hitOk, `${label} sidebar scrim close point ${JSON.stringify(scrim)}`).toBe(true);
}

async function clickSidebarScrimAndAssertClosed(page: Page, label: string): Promise<void> {
  const point = await page.evaluate(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer.is-open");
    const backdrop = document.querySelector(".ct-sidebar-backdrop.is-open");
    if (!drawer || !backdrop) return null;
    const drawerRect = drawer.getBoundingClientRect();
    return {
      x: Math.min(document.documentElement.clientWidth - 8, Math.max(drawerRect.right + 8, 8)),
      y: Math.min(document.documentElement.clientHeight - 8, Math.max(drawerRect.top + 24, 8)),
    };
  });
  expect(point, `${label} sidebar scrim click point`).not.toBeNull();
  await page.mouse.click(point!.x, point!.y);
  await page.waitForFunction(() => {
    const drawer = document.querySelector(".ct-sidebar-drawer");
    const backdrop = document.querySelector(".ct-sidebar-backdrop");
    if (!drawer) return true;
    const drawerStyle = window.getComputedStyle(drawer);
    const backdropStyle = backdrop ? window.getComputedStyle(backdrop) : null;
    return !drawer.classList.contains("is-open") &&
      drawer.getAttribute("aria-hidden") === "true" &&
      drawer.hasAttribute("inert") &&
      drawerStyle.pointerEvents === "none" &&
      (!backdrop || (
        backdrop.getAttribute("aria-hidden") === "true" &&
        backdropStyle?.pointerEvents === "none"
      ));
  }, null, { timeout: 5_000 });
  await settleLayout(page);
  await assertPeerPickerReachable(page, `${label} after scrim close`);
}

async function auditSidebarDrawer(page: Page, label: string, expectedOpen: boolean): Promise<void> {
  if (expectedOpen) {
    const drawer = page.locator(".ct-sidebar-drawer.is-open").first();
    try {
      await drawer.waitFor({ state: "attached", timeout: 5_000 });
    } catch (error) {
      throw new Error(`${label} drawer-never-opened: .ct-sidebar-drawer.is-open did not attach within 5000ms`, {
        cause: error,
      });
    }
    const result = await drawer.evaluate((el) => {
      const rect = el.getBoundingClientRect();
      const style = window.getComputedStyle(el);
      const viewportWidth = document.documentElement.clientWidth;
      return {
        open: el.classList.contains("is-open"),
        ariaHidden: el.getAttribute("aria-hidden"),
        inert: el.hasAttribute("inert"),
        pointerEvents: style.pointerEvents,
        left: rect.left,
        right: rect.right,
        width: rect.width,
        viewportWidth,
        visibleWidth: Math.max(0, Math.min(rect.right, viewportWidth) - Math.max(rect.left, 0)),
        maxAllowed: viewportWidth * 0.85 + 1,
      };
    });
    expect(result.open, `${label} drawer open state`).toBe(true);
    expect(result.ariaHidden, `${label} open drawer aria-hidden`).toBe("false");
    expect(result.inert, `${label} open drawer inert`).toBe(false);
    expect(result.pointerEvents, `${label} open drawer pointer events`).toBe("auto");
    expect(result.left, `${label} open drawer left`).toBeGreaterThanOrEqual(-1);
    expect(result.visibleWidth, `${label} open drawer visible width`).toBeLessThanOrEqual(result.maxAllowed);
    expect(result.right, `${label} open drawer right`).toBeLessThanOrEqual(result.viewportWidth + 1);
    return;
  }

  const result = await page.evaluate((interactiveSelector) => {
    const drawer = document.querySelector(".ct-sidebar-drawer") as HTMLElement | null;
    if (!drawer) return { present: false as const };
    const rect = drawer.getBoundingClientRect();
    const style = window.getComputedStyle(drawer);
    const firstInteractive = drawer.querySelector(interactiveSelector) as HTMLElement | null;
    firstInteractive?.focus();
    const backdrop = document.querySelector(".ct-sidebar-backdrop") as HTMLElement | null;
    const backdropStyle = backdrop ? window.getComputedStyle(backdrop) : null;
    const x = Math.min(Math.max(rect.left + Math.max(rect.width, 1) / 2, 0), document.documentElement.clientWidth - 1);
    const y = Math.min(Math.max(rect.top + Math.max(rect.height, 1) / 2, 0), document.documentElement.clientHeight - 1);
    const hit = document.elementFromPoint(x, y);
    return {
      present: true as const,
      open: drawer.classList.contains("is-open"),
      ariaHidden: drawer.getAttribute("aria-hidden"),
      inert: drawer.hasAttribute("inert"),
      pointerEvents: style.pointerEvents,
      right: rect.right,
      focusedInside: drawer.contains(document.activeElement),
      hitInside: !!hit && drawer.contains(hit),
      backdrop: backdrop
        ? {
            ariaHidden: backdrop.getAttribute("aria-hidden"),
            pointerEvents: backdropStyle?.pointerEvents,
          }
        : null,
    };
  }, INTERACTIVE);
  await expect(page.locator('[data-testid="web-sidebar-open"]'), `${label} closed drawer trigger`).toBeVisible();
  if (!result.present) return;

  expect(result.open, `${label} drawer open state`).toBe(false);
  expect(result.ariaHidden, `${label} closed drawer aria-hidden`).toBe("true");
  expect(result.inert, `${label} closed drawer inert`).toBe(true);
  expect(result.pointerEvents, `${label} closed drawer pointer events`).toBe("none");
  expect(result.right, `${label} closed drawer offscreen`).toBeLessThanOrEqual(1);
  expect(result.focusedInside, `${label} closed drawer focus trap`).toBe(false);
  expect(result.hitInside, `${label} closed drawer hit target`).toBe(false);
  if (result.backdrop) {
    expect(result.backdrop.ariaHidden, `${label} closed backdrop aria-hidden`).toBe("true");
    expect(result.backdrop.pointerEvents, `${label} closed backdrop pointer events`).toBe("none");
  }
}

test("@golden mobile web primary surfaces stay inside the viewport audit helper self-check", async ({ webBrowser }, testInfo) => {
  const healthJson = await assertHealthMatchesHead();
  await testInfo.attach("health.json", { body: healthJson, contentType: "application/json" });

  const page = await newWebPage(webBrowser, { width: 320, height: 568 });
  try {
    await page.setContent(`
      <style>
        html, body, #root { width: 100%; height: 100%; margin: 0; overflow: hidden; }
        #scroll-owner { position: absolute; left: 8px; top: 8px; width: 140px; height: 72px; overflow-y: hidden; overflow-x: visible; }
        #clipped-descendant { margin-top: 120px; }
        #offscreen-unowned { position: absolute; left: 360px; top: 24px; width: 80px; height: 32px; }
        #partial-unowned { position: absolute; left: 292px; top: 96px; width: 80px; height: 32px; }
        #covered-clickthrough { position: absolute; left: 40px; top: 180px; width: 120px; height: 32px; }
        #floating-cover { position: absolute; left: 40px; top: 180px; width: 120px; height: 32px; z-index: 2; background: rgba(0, 0, 0, 0.2); }
        #inactive-layer { position: absolute; left: 40px; top: 240px; width: 160px; height: 48px; }
        #inactive-layer-button { width: 120px; height: 32px; }
      </style>
      <div id="root">
        <div id="scroll-owner">
          <button id="clipped-descendant" data-testid="clipped-descendant">clipped</button>
        </div>
        <button id="offscreen-unowned" data-testid="offscreen-unowned">offscreen</button>
        <button id="partial-unowned" data-testid="partial-unowned">partial</button>
        <button id="covered-clickthrough" data-testid="covered-clickthrough">covered</button>
        <div id="floating-cover" data-testid="floating-cover"></div>
        <div id="inactive-layer" inert aria-hidden="true">
          <button id="inactive-layer-button" data-testid="inactive-layer-button">inactive</button>
        </div>
      </div>
    `);

    const offenders = await collectViewportOffenders(page);
    const ids = offenders.map((item) => item?.testid).sort();
    const ancestorOnlyIds = await page.locator(INTERACTIVE).evaluateAll((nodes) => {
      const viewportWidth = document.documentElement.clientWidth;
      const viewportHeight = document.documentElement.clientHeight;
      return nodes
        .map((node) => {
          const el = node as HTMLElement;
          const rect = el.getBoundingClientRect();
          const inBounds =
            rect.left >= -1 &&
            rect.right <= viewportWidth + 1 &&
            rect.top >= -1 &&
            rect.bottom <= viewportHeight + 1;
          return inBounds ? null : el.getAttribute("data-testid");
        })
        .filter(Boolean);
    });
    expect(ids).not.toContain("clipped-descendant");
    expect(ids).toContain("offscreen-unowned");
    expect(ids).toContain("partial-unowned");
    expect(ids).not.toContain("inactive-layer-button");
    expect(ancestorOnlyIds).not.toContain("covered-clickthrough");
    expect(ids).toContain("covered-clickthrough");
    const collectorSource = collectViewportOffenders.toString();
    const helperSource = readFileSync("tests/playwright/helpers/mobile-overflow.ts", "utf8");
    const specSource = readFileSync("tests/playwright/suites/12-web/mobile-overflow.spec.ts", "utf8");
    expect(collectorSource).toContain('getAttribute("aria-hidden") === "true"');
    expect(collectorSource).not.toContain("pointerEvents");
    expect(collectorSource).not.toContain("opacity");
    expect(helperSource).toContain("export async function revealOwnedHorizontalSurface");
    expect(specSource).toContain("revealOwnedHorizontalSurface, type ReachabilityMode");
    expect(specSource).toContain("await revealOwnedHorizontalSurface(page, meshTab");
    expect(specSource).toContain("owner.scrollLeft + viewport - ownerRect.left - width / 2");
    const peerDismissSource = dismissPeerPickerWithSupportedControl.toString();
    expect(peerDismissSource).toContain('[data-testid="peer-picker-button"]');
    expect(peerDismissSource).toContain(".click()");
    expect(peerDismissSource).toContain('[data-testid="peer-picker-menu"]');
    expect(peerDismissSource).toMatch(/state:\s*"hidden",\s*timeout:\s*5_?000/);
    expect(peerDismissSource).not.toContain("Escape");
    expect(peerDismissSource).not.toContain(".catch(");

    await page.setContent(`
      <style>
        html, body, #root { width: 100%; height: 100%; margin: 0; overflow: hidden; }
        .ct-sidebar-backdrop { position: fixed; inset: 0; z-index: 30; pointer-events: auto; }
        .ct-sidebar-drawer { position: fixed; left: 0; top: 0; bottom: 0; width: 220px; z-index: 40; background: white; }
        #covered-background { position: absolute; left: 260px; top: 20px; width: 48px; height: 32px; }
        #drawer-control { position: absolute; left: 16px; top: 20px; width: 96px; height: 32px; }
      </style>
      <div id="root">
        <button id="covered-background" data-testid="covered-background">covered</button>
        <div class="ct-sidebar-backdrop is-open" data-testid="overlay-scrim"></div>
        <div class="ct-sidebar-drawer is-open" data-testid="overlay-drawer">
          <button id="drawer-control" data-testid="drawer-control">drawer</button>
        </div>
      </div>
    `);
    const overlayOffenders = await collectViewportOffenders(page, "open-drawer");
    const overlayIds = overlayOffenders.map((item) => item?.testid).sort();
    expect(overlayIds).not.toContain("covered-background");
    expect(overlayIds).not.toContain("drawer-control");
    const backgroundHit = await page.locator('[data-testid="covered-background"]').evaluate((el) => {
      const rect = el.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return {
        reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
        hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      };
    });
    expect(backgroundHit).toEqual({ reachable: false, hitTestid: "overlay-scrim" });
    await assertSidebarScrimReachable(page, "overlay self-check");

    await page.evaluate(() => {
      const drawer = document.querySelector(".ct-sidebar-drawer");
      const cover = document.createElement("div");
      cover.dataset.testid = "drawer-cover";
      cover.style.position = "absolute";
      cover.style.left = "16px";
      cover.style.top = "20px";
      cover.style.width = "96px";
      cover.style.height = "32px";
      cover.style.zIndex = "1";
      drawer?.appendChild(cover);
    });
    const blockedDrawerOffenders = await collectViewportOffenders(page, "open-drawer");
    const blockedDrawer = blockedDrawerOffenders.find((item) => item?.testid === "drawer-control");
    expect(blockedDrawer?.hitTestid).toBe("drawer-cover");

    await page.setContent(`
      <style>
        html, body, #root { width: 100%; height: 100%; margin: 0; overflow: hidden; }
        #popup-occluded { position: absolute; left: 40px; top: 32px; width: 120px; height: 32px; }
        #popup-unrelated { position: absolute; left: 40px; top: 112px; width: 120px; height: 32px; }
        #unrelated-cover { position: absolute; left: 40px; top: 112px; width: 120px; height: 32px; z-index: 2; }
        [data-testid="peer-picker-menu"] { position: absolute; left: 32px; top: 24px; width: 160px; height: 72px; z-index: 3; background: white; }
        #popup-contained { position: absolute; left: 8px; top: 36px; width: 120px; height: 32px; }
      </style>
      <div id="root">
        <button id="popup-occluded" data-testid="popup-occluded">occluded</button>
        <button id="popup-unrelated" data-testid="popup-unrelated">unrelated</button>
        <div id="unrelated-cover" data-testid="unrelated-cover"></div>
        <div data-testid="peer-picker-menu">
          <button id="popup-contained" data-testid="popup-contained">contained</button>
        </div>
      </div>
    `);
    const popupOffenders = await collectViewportOffenders(page, "peer-picker-menu");
    const popupIds = popupOffenders.map((item) => item?.testid).sort();
    expect(popupIds).not.toContain("popup-contained");
    expect(popupIds).not.toContain("popup-occluded");
    expect(popupIds).toContain("popup-unrelated");
    const popupOccludedHit = await page.locator('[data-testid="popup-occluded"]').evaluate((el) => {
      const rect = el.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return {
        reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
        hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      };
    });
    expect(popupOccludedHit).toEqual({ reachable: false, hitTestid: "peer-picker-menu" });

    await page.setContent(`<style>html,body{margin:0;overflow:hidden}.ct-float{position:fixed;left:32px;top:24px;width:160px;height:72px;z-index:3}button{position:absolute;width:120px;height:32px}#workspace-covered{left:40px;top:32px}#workspace-unrelated,#workspace-cover{left:40px;top:112px}#workspace-cover{position:absolute;width:120px;height:32px;z-index:2}</style><div data-testid="new-session-picker"><div role="dialog" aria-modal="true"><button id="workspace-covered" data-testid="workspace-covered">covered</button><button id="workspace-unrelated" data-testid="workspace-unrelated">unrelated</button><div id="workspace-cover"></div></div></div><div class="ct-float"><button data-testid="picker-workspace-option-x">option</button></div>`);
    const workspaceCoverHit = await page.locator("#workspace-unrelated").evaluate((el) => {
      const rect = el.getBoundingClientRect();
      return document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2)?.id;
    });
    expect(workspaceCoverHit, "workspace unrelated precondition is occluded by its intended cover").toBe("workspace-cover");
    const workspaceIds = (await collectViewportOffenders(page, "workspace-dropdown")).map((item) => item?.testid);
    expect(workspaceIds).not.toContain("picker-workspace-option-x");
    expect(workspaceIds).not.toContain("workspace-covered");
    expect(workspaceIds).toContain("workspace-unrelated");
    const workspaceDismissSource = dismissWorkspaceDropdownWithSupportedControl.toString();
    expect(workspaceDismissSource).toContain('[data-testid="picker-workspace"]');
    expect(workspaceDismissSource).not.toContain("Escape");
    expect(workspaceDismissSource).not.toContain(".catch(");

    await page.setContent(`<style>html,body{margin:0;overflow:hidden}.ct-hscroll{position:relative;width:320px;overflow-x:auto;white-space:nowrap}.ct-hscroll button{width:80px}.ct-hscroll #mesh-tab-network{margin-left:360px}.ct-hscroll #owned-partial-sibling{position:absolute;left:400px}</style><div class="ct-hscroll"><button data-testid="mesh-tab-network" id="mesh-tab-network">Network</button><button data-testid="owned-partial-sibling" id="owned-partial-sibling">Sibling</button></div>`);
    const beforeScroll = await page.locator('[data-testid="mesh-tab-network"]').evaluate((el) => el.getBoundingClientRect().right > document.documentElement.clientWidth);
    expect(beforeScroll, "known surface starts outside the viewport before reveal").toBe(true);
    await auditKnownSurfaces(page, "hscroll self-check");
    expect(await page.locator(".ct-hscroll").evaluate((el) => el.scrollLeft), "owned hscroll revealed Network").toBeGreaterThan(0);
    await page.locator("#owned-partial-sibling").evaluate((el) => {
      const owner = el.closest(".ct-hscroll") as HTMLElement | null;
      if (!owner) throw new Error("owned sibling has no hscroll owner");
      const ownerRect = owner.getBoundingClientRect();
      const viewport = document.documentElement.clientWidth;
      const width = el.getBoundingClientRect().width;
      (el as HTMLElement).style.left = `${owner.scrollLeft + viewport - ownerRect.left - width / 2}px`;
    });
    const ownedSibling = await page.locator('[data-testid="owned-partial-sibling"]').evaluate((el) => {
      const rect = el.getBoundingClientRect();
      return { left: rect.left, right: rect.right, viewport: document.documentElement.clientWidth };
    });
    expect(ownedSibling.left, "owned sibling starts inside the viewport").toBeGreaterThanOrEqual(0);
    expect(ownedSibling.right, "owned sibling is partially clipped horizontally").toBeGreaterThan(ownedSibling.viewport);
    expect((await collectViewportOffenders(page)).map((item) => item?.testid)).not.toContain("owned-partial-sibling");
    await page.setContent(`<button data-testid="non-scroller-partial" style="position:absolute;left:292px;width:80px">Non-scroller</button>`);
    expect((await collectViewportOffenders(page)).map((item) => item?.testid)).toContain("non-scroller-partial");
    await page.setContent(`<button data-testid="mesh-tab-network" style="position:absolute;left:360px">Network</button>`);
    await expect(auditKnownSurfaces(page, "unowned hscroll self-check")).rejects.toThrow(/right/);

    const popupCollectorSource = collectViewportOffenders.toString();
    expect(popupCollectorSource).toContain('mode === "peer-picker-menu"');
    expect(popupCollectorSource).toContain("inPeerPickerMenu ? reachable");
    const popupLayerSource = assertPeerPickerMenuInverseOcclusion.toString();
    expect(popupLayerSource).toContain("inverse-occlusion peer menu covers Chat and Terminal tabs");
    expect(targetTabModeSwitch.toString()).toContain('[data-tab-id="${tabId}"]');
    expect(targetTabModeSwitch.toString()).toContain('[data-testid="tab-mode-switch"]');
    expect(assertTabModeTabsReachable.toString()).toContain("target tab mode switch count");
    expect(assertPeerPickerMenuInverseOcclusion.toString()).toContain("inverse-occlusion target tab mode switch count");

    await page.setContent(`
      <style>
        html, body, #root { width: 100%; height: 100%; margin: 0; overflow: hidden; }
        #settings-occluded { position: absolute; left: 40px; top: 32px; width: 120px; height: 32px; }
        #settings-unrelated { position: absolute; left: 40px; top: 112px; width: 120px; height: 32px; }
        #settings-unrelated-cover { position: absolute; left: 40px; top: 112px; width: 120px; height: 32px; z-index: 2; }
        [data-testid="settings-menu"] { position: absolute; left: 32px; top: 24px; width: 160px; height: 72px; z-index: 3; background: white; }
        #settings-contained { position: absolute; left: 8px; top: 36px; width: 120px; height: 32px; }
      </style>
      <div id="root">
        <button id="settings-occluded" data-testid="settings-occluded">occluded</button>
        <button id="settings-unrelated" data-testid="settings-unrelated">unrelated</button>
        <div id="settings-unrelated-cover" data-testid="settings-unrelated-cover"></div>
        <div data-testid="settings-menu">
          <button id="settings-contained" data-testid="settings-contained">contained</button>
        </div>
      </div>
    `);
    const settingsMenuOffenders = await collectViewportOffenders(page, "settings-menu");
    const settingsMenuIds = settingsMenuOffenders.map((item) => item?.testid).sort();
    expect(settingsMenuIds).not.toContain("settings-contained");
    expect(settingsMenuIds).not.toContain("settings-occluded");
    expect(settingsMenuIds).toContain("settings-unrelated");
    const settingsOccludedHit = await page.locator('[data-testid="settings-occluded"]').evaluate((el) => {
      const rect = el.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return {
        reachable: hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el)),
        hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
      };
    });
    expect(settingsOccludedHit).toEqual({ reachable: false, hitTestid: "settings-menu" });

    const settingsCollectorSource = collectViewportOffenders.toString();
    expect(settingsCollectorSource).toContain('mode === "settings-menu"');
    expect(settingsCollectorSource).toContain("inSettingsMenu ? reachable");
    const auditSource = auditViewport.toString();
    expect(auditSource).toContain('[data-testid="settings-menu"]');
    expect(auditSource).toMatch(/state:\s*"visible",\s*timeout:\s*5_?000/);

    await page.setContent(`
      <style>
        html, body, #root { width: 100%; height: 100%; margin: 0; overflow: hidden; }
        [data-testid="tab-mode-switch"] { position: absolute; left: 40px; top: 40px; display: flex; gap: 8px; }
        [data-tab-id="foreign-tab"] [data-testid="tab-mode-switch"] { top: 160px; }
        [role="tab"] { width: 80px; height: 32px; }
        [data-testid="peer-picker-menu"] { position: absolute; left: 32px; top: 32px; width: 196px; height: 52px; z-index: 2; background: white; }
      </style>
      <div id="root">
        <div data-tab-id="target-tab">
          <div data-testid="tab-mode-switch">
            <button role="tab">Chat</button>
            <button role="tab">Terminal</button>
          </div>
        </div>
        <div data-tab-id="foreign-tab">
          <div data-testid="tab-mode-switch">
            <button role="tab">Chat</button>
            <button role="tab">Terminal</button>
          </div>
        </div>
        <div data-testid="peer-picker-menu"></div>
      </div>
    `);
    await assertPeerPickerMenuInverseOcclusion(page, "target-tab", "tab cardinality self-check");
    await page.locator('[data-testid="peer-picker-menu"]').evaluate((el) => el.remove());
    await assertTabModeTabsReachable(page, "target-tab", "tab cardinality self-check");
    await page.locator('[data-tab-id="target-tab"]').evaluate((target) => {
      const duplicate = document.createElement("div");
      duplicate.dataset.testid = "tab-mode-switch";
      for (const label of ["Chat", "Terminal"]) {
        const tab = document.createElement("button");
        tab.setAttribute("role", "tab");
        tab.textContent = label;
        duplicate.appendChild(tab);
      }
      target.appendChild(duplicate);
    });
    await expect(
      assertTabModeTabsReachable(page, "target-tab", "duplicate tab cardinality self-check"),
    ).rejects.toThrow(/target tab mode switch count/);
    await expect(
      assertPeerPickerMenuInverseOcclusion(page, "target-tab", "duplicate tab cardinality self-check"),
    ).rejects.toThrow(/inverse-occlusion target tab mode switch count/);
  } finally {
    await page.context().close();
  }
});

for (const vp of VIEWPORTS) {
  test(`@golden mobile web primary surfaces stay inside the viewport ${vp.name}`, async ({ webBrowser, api, tabCleanup, spawnAgent }, testInfo) => {
    const health = await assertHealthMatchesHead();
    await testInfo.attach("health.json", { body: JSON.stringify(health, null, 2), contentType: "application/json" });

    // The layout assertions need the chat surface mounted, so the fixture owns
    // a spawned tab and waits for its visible ready state before auditing layout.
    const target = await prepareTargetTab(api, tabCleanup, false, spawnAgent);
    const page = await newWebPage(webBrowser, vp);
    try {
      await openApp(page);
      await assertBuildIdentity(page, health);
      await settleLayout(page);
      await openSidebarAndSelectTargetTab(api, page, target, `${vp.name} initial target`);
      await assertTargetChatReady(page, target, vp.name);
      await openWorkspaceCanvasOverlay(page, `${vp.name} before tab transition`);
      await assertCanvasStateBeforeTabTransition(page, target.tabId, "overlay", `${vp.name} before tab transition`);
      await openSidebarAndSelectTargetTab(api, page, target, `${vp.name} canvas-to-workspace`);
      await assertWorkspaceLayerTopmost(page, target.tabId, `${vp.name} after target row click`);
      await assertPeerPickerTopContained(page, `${vp.name} initial`);
      await auditViewport(page, `${vp.name} initial`);
      await auditSidebarDrawer(page, `${vp.name} closed sidebar`, false);

      await openSidebar(page);
      await auditSidebarDrawer(page, `${vp.name} open sidebar`, true);
      await auditViewport(page, `${vp.name} open sidebar`, { reachability: "open-drawer" });
      await clickSidebarScrimAndAssertClosed(page, `${vp.name} open sidebar`);
      await auditSidebarDrawer(page, `${vp.name} closed sidebar after dismiss`, false);

      await page.locator('[data-testid="peer-picker-button"]').click();
      await page.locator('[data-testid="peer-picker-menu"]').waitFor({ state: "visible", timeout: 5_000 });
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} peer menu`);
      await auditViewport(page, `${vp.name} peer menu`, { reachability: "peer-picker-menu" });
      await assertPeerPickerMenuInverseOcclusion(page, target.tabId, `${vp.name} peer menu`);
      await dismissPeerPickerWithSupportedControl(page, `${vp.name} peer menu`);
      await assertTabModeTabsReachable(page, target.tabId, `${vp.name} peer menu dismissed`);

      await page.locator('[data-testid="titlebar-settings"]').click();
      await page.locator('[data-testid="settings-menu"]').waitFor({ state: "visible", timeout: 5_000 });
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} settings menu`);
      await auditViewport(page, `${vp.name} settings menu`, { reachability: "settings-menu" });
      await page.locator('[data-testid="settings-menu-general"]').click();
      await page.locator('[data-testid="settings-panel"]').waitFor({ state: "visible", timeout: 10_000 });
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} settings panel`);
      await auditViewport(page, `${vp.name} settings panel`);
      await page.locator('[data-testid="settings-close"]').click();

      await openSidebar(page);
      await auditSidebarDrawer(page, `${vp.name} open sidebar`, true);
      await auditViewport(page, `${vp.name} sidebar tree`, { reachability: "open-drawer" });
      const newSession = page.locator('[data-testid="sidebar-new-session"]').first();
      if (await newSession.isVisible().catch(() => false)) {
        await newSession.click();
        await page.locator('[data-testid="new-session-picker"]').waitFor({ state: "visible", timeout: 10_000 });
        const newSessionDialog = page.getByRole("dialog", { name: "Start a session" });
        await expect(newSessionDialog).toBeVisible({ timeout: 10_000 });
        await expect(newSessionDialog).toHaveAttribute("aria-modal", "true");
        await settleLayout(page);
        await auditKnownSurfaces(page, `${vp.name} new session picker`);
        await auditViewport(page, `${vp.name} new session picker`, { reachability: "new-session-picker" });
        const workspaceSelect = page.locator('[data-testid="picker-workspace"]').first();
        if (await workspaceSelect.isVisible().catch(() => false)) {
          await workspaceSelect.click();
          await page.locator('[data-testid^="picker-workspace-option-"]').first().waitFor({ state: "visible", timeout: 5_000 });
          await settleLayout(page);
          await auditViewport(page, `${vp.name} picker workspace dropdown`, { reachability: "workspace-dropdown" });
          await dismissWorkspaceDropdownWithSupportedControl(page, `${vp.name} picker workspace dropdown`);
        }
        await newSessionDialog.getByRole("button", { name: "Close new session dialog" }).click();
        await expect(newSessionDialog).toHaveCount(0);
      }

      await openSidebar(page);
      await page.locator('[data-testid="nav-mesh"]').click();
      const meshTab = page.locator('[data-testid="mesh-tab-network"]');
      await meshTab.waitFor({ state: "visible", timeout: 10_000 });
      // The mesh section no longer owns a horizontal scroller: `.ct-section-toolbar`
      // WRAPS below 640px and gives the tab rail the full width (globals.css), so
      // the rail must already be whole inside the viewport with nothing to reveal.
      const meshToolbar = page.locator(".ct-section-toolbar").filter({ has: meshTab });
      await expect(meshToolbar, `${vp.name} mesh exact toolbar owner`).toHaveCount(1);
      await expect(
        page.locator(".ct-hscroll").filter({ has: meshTab }),
        `${vp.name} mesh rail must not need a horizontal scroller`,
      ).toHaveCount(0);
      await revealOwnedHorizontalSurface(page, meshTab, `${vp.name} mesh`);
      const railBox = await meshTab.boundingBox();
      expect(railBox, `${vp.name} mesh rail box`).toBeTruthy();
      if (railBox) {
        expect(railBox.x, `${vp.name} mesh rail left`).toBeGreaterThanOrEqual(-1);
        expect(railBox.x + railBox.width, `${vp.name} mesh rail right`).toBeLessThanOrEqual(vp.width + 1);
      }
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} mesh`);
      await auditViewport(page, `${vp.name} mesh`);

      await page.screenshot({ path: testInfo.outputPath(`mobile-overflow-${vp.name}.png`), fullPage: false });
    } finally {
      await page.context().close();
      if (target.planId) await api.deletePlan(target.planId).catch(() => {});
      if (target.fixtureWorkspaceId) await api.closeWorkspace(target.fixtureWorkspaceId).catch(() => {});
    }
  });
}

test("@golden mobile-overflow chat readiness fails when absent", async ({ webBrowser }) => {
  const page = await newWebPage(webBrowser, VIEWPORTS[0]);
  try {
    await page.setContent("<main></main>");
    await expect(
      assertTargetChatReady(page, { tabId: "missing-tab", tabTitle: "missing tab" }, "missing readiness"),
    ).rejects.toThrow();
  } finally {
    await page.context().close();
  }
});

// A Team's managers section additionally needs an anchor role and its verified
// live session, so it remains split from the ordinary chat-surface viewports.
test("@golden mobile web Team managers stay readable in the phone drawer", async ({ webBrowser, api, tabCleanup, spawnAgent }) => {
  test.skip(
    !(await isStubTransportActive(api)),
    "dev daemon is not running with CODETERM_STUB_AGENT_CMD",
  );
  const vp = { width: 390, height: 844, name: "390x844" } as const;
  const target = await prepareTargetTab(api, tabCleanup, true, spawnAgent);
  const page = await newWebPage(webBrowser, vp);
  try {
    await openApp(page);
    await settleLayout(page);
    await openSidebarAndSelectTargetTab(api, page, target, `${vp.name} initial target`);
    await openSidebar(page);
    await assertActiveManagerReadable(api, page, target, `${vp.name} Team manager`);
    await openSidebar(page);
    await auditSidebarDrawer(page, `${vp.name} Team manager drawer`, true);
    await auditViewport(page, `${vp.name} Team manager drawer`, { reachability: "open-drawer" });
  } finally {
    await page.context().close();
    if (target.planId) await api.deletePlan(target.planId).catch(() => {});
    if (target.fixtureWorkspaceId) await api.closeWorkspace(target.fixtureWorkspaceId).catch(() => {});
  }
});
