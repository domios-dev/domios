/**
 * WHAT: mobile overflow covers broad web surfaces at phone widths.
 * WHY: 320px regressions have repeatedly come from fixed-width panels, menus,
 * and dialogs rather than from the terminal/code scroll surfaces themselves.
 * ORACLE: visible surfaces and controls are fully inside the viewport and
 * reachable at their center; document scroll width is only a secondary signal.
 */
import { execFileSync } from "node:child_process";
import { expect, test, type Browser, type Page, type PaneCleanup, API_BASE, readDevToken } from "../../helpers/fixtures";
import type { ApiClient } from "../../helpers/api-client";

const VIEWPORTS = [
  { width: 320, height: 568, name: "320x568" },
  { width: 390, height: 844, name: "390x844" },
  { width: 430, height: 932, name: "430x932" },
] as const;

const INTERACTIVE = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  '[role="button"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="switch"]',
  '[tabindex]:not([tabindex="-1"])',
].join(",");

const OPTIONAL_SURFACES = [
  '[data-testid="new-session-picker"]',
  '[data-testid="settings-panel"]',
  '[data-testid="mesh-tab-network"]',
  '[data-testid="peer-picker-menu"]',
  '[data-testid="settings-menu"]',
  '[data-testid="sidebar"]',
] as const;

type ReachabilityMode = "all" | "open-drawer";
type CanvasLayerExpectation = "overlay" | "inline";

type CreatedPane = { id: string; tab_id?: string };
type TargetPane = { paneId: string; tabId: string; workspaceId: string };
type MuxState = {
  workspaces: Array<{
    id: string;
    tabs: Array<{ id: string; pane_id: string }>;
  }>;
};

async function newWebPage(browser: Browser, viewport: { width: number; height: number }): Promise<Page> {
  const ctx = await browser.newContext({ viewport });
  return ctx.newPage();
}

async function openApp(page: Page): Promise<void> {
  await page.goto(`${API_BASE}/#token=${readDevToken()}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 20_000 });
}

async function openSidebar(page: Page): Promise<void> {
  const burger = page.locator('[data-testid="web-sidebar-open"]');
  if (await burger.isVisible().catch(() => false)) {
    await burger.click();
  }
  await expect(page.locator(".ct-sidebar-drawer.is-open"), "drawer-never-opened").toBeAttached({ timeout: 5_000 });
  await page.locator('[data-testid="workspace-item"]').first().waitFor({ state: "visible", timeout: 10_000 });
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

async function assertHealthMatchesHead(): Promise<string> {
  const expected = process.env.CT_E2E_EXPECTED_BUILD_SHA ?? execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const health = await fetch(`${API_BASE}/api/health`, { signal: AbortSignal.timeout(5_000) }).then((r) => r.json()) as {
    build_sha?: string;
    build?: string;
  };
  expect(health.build_sha, `health build_sha ${JSON.stringify(health)}`).toBe(expected);
  return JSON.stringify(health, null, 2);
}

async function collectViewportOffenders(page: Page, reachabilityMode: ReachabilityMode = "all") {
  return page.locator(INTERACTIVE).evaluateAll((nodes, mode) => {
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const openDrawer = mode === "open-drawer"
      ? document.querySelector(".ct-sidebar-drawer.is-open")
      : null;
    const openBackdrop = mode === "open-drawer"
      ? document.querySelector(".ct-sidebar-backdrop.is-open")
      : null;
    const clips = (value: string) => /auto|scroll|hidden|clip/.test(value);
    const isFullyClippedByAncestor = (el: HTMLElement, rect: DOMRect) => {
      let parent = el.parentElement;
      while (parent && parent !== document.body && parent !== document.documentElement) {
        if (parent.id === "root") {
          parent = parent.parentElement;
          continue;
        }
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
    const hasInactiveSemantics = (el: HTMLElement) => {
      let current: HTMLElement | null = el;
      while (current && current !== document.body && current !== document.documentElement) {
        if (current.hasAttribute("inert") || current.getAttribute("aria-hidden") === "true") return true;
        current = current.parentElement;
      }
      return false;
    };
    return nodes
      .map((node) => {
        const el = node as HTMLElement;
        const style = window.getComputedStyle(el);
        const rect = el.getBoundingClientRect();
        const hidden =
          style.visibility === "hidden" ||
          style.display === "none" ||
          rect.width <= 0 ||
          rect.height <= 0 ||
          isFullyClippedByAncestor(el, rect) ||
          hasInactiveSemantics(el) ||
          el.closest(".xterm-accessibility, .xterm-helper-textarea") !== null;
        if (hidden) return null;
        const left = rect.left;
        const right = rect.right;
        const top = rect.top;
        const bottom = rect.bottom;
        const centerX = Math.min(Math.max(left + rect.width / 2, 0), viewportWidth - 1);
        const centerY = Math.min(Math.max(top + rect.height / 2, 0), viewportHeight - 1);
        const hit = document.elementFromPoint(centerX, centerY);
        const reachable = hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el));
        const inOpenDrawer = !!openDrawer?.contains(el);
        const hitInOpenLayer = hit instanceof Node &&
          (!!openDrawer?.contains(hit) || hit === openDrawer || !!openBackdrop?.contains(hit) || hit === openBackdrop);
        const inBounds =
          left >= -1 &&
          right <= viewportWidth + 1 &&
          top >= -1 &&
          bottom <= viewportHeight + 1;
        const layerReachabilityOk =
          mode === "all"
            ? reachable
            : inOpenDrawer
              ? reachable
              : !reachable && hitInOpenLayer;
        if (inBounds && layerReachabilityOk) return null;
        return {
          tag: el.tagName.toLowerCase(),
          testid: el.getAttribute("data-testid"),
          role: el.getAttribute("role"),
          title: el.getAttribute("title"),
          text: (el.innerText || el.textContent || "").trim().slice(0, 80),
          left,
          top,
          right,
          bottom,
          width: rect.width,
          height: rect.height,
          viewportWidth,
          viewportHeight,
          reachable,
          inOpenDrawer,
          hitInOpenLayer,
          hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
          hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
        };
      })
      .filter(Boolean);
  }, reachabilityMode);
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
  }
  const offenders = await collectViewportOffenders(page, options.reachability ?? "all");
  expect(offenders, `${label} offscreen or unreachable controls`).toEqual([]);
  if (options.reachability === "open-drawer") {
    await assertOpenDrawerControlsReachable(page, label);
    await assertSidebarScrimReachable(page, label);
  }
}

async function prepareTargetPane(api: ApiClient, paneCleanup: PaneCleanup): Promise<TargetPane> {
  const pane = await paneCleanup.createPane<CreatedPane>({ title: `pw-mobile-layer-${Date.now()}` });
  await api.post(`/panes/${pane.id}/agent-type`, { agent_type: "claude" });
  await api.post(`/panes/${pane.id}/chat-view`, { enabled: true });
  await api.focusPane(pane.id);

  const mux = await api.get<MuxState>("/mux/state");
  for (const workspace of mux.workspaces) {
    const tab = workspace.tabs.find((candidate) => candidate.pane_id === pane.id);
    if (tab) return { paneId: pane.id, tabId: tab.id, workspaceId: workspace.id };
  }
  throw new Error(`created pane ${pane.id} has no owning tab in mux state`);
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

async function openSidebarAndSelectTargetPane(api: ApiClient, page: Page, target: TargetPane, label: string): Promise<void> {
  await api.focusWorkspace(target.workspaceId);
  await openSidebar(page);

  const group = page.locator(`[data-testid="workspace-group"][data-workspace-id="${target.workspaceId}"]`).first();
  await expect(group, `${label} target workspace group visible`).toBeVisible({ timeout: 10_000 });
  const tab = group.locator(`[data-testid="tab-item"][data-tab-id="${target.tabId}"]`).first();
  if (!(await tab.isVisible().catch(() => false))) {
    await group.locator('button[title="Expand"]').first().click();
  }
  await expect(tab, `${label} target pane owning data-tab-id row visible`).toBeVisible({ timeout: 10_000 });
  await tab.click();
  await waitSidebarClosed(page);
}

async function openWorkspaceCanvasOverlay(page: Page, label: string): Promise<void> {
  const button = page.locator('[data-testid="canvas-toolbar-button"]').first();
  await expect(button, `${label} canvas toolbar button visible`).toBeVisible({ timeout: 10_000 });
  if ((await button.getAttribute("aria-pressed")) !== "true") await button.click();
  await expect(
    page.locator('[data-testid="canvas-view"], [data-testid="plan-view-shell"], [data-testid="canvas-empty"]').first(),
    `${label} workspace canvas visible`,
  ).toBeVisible({ timeout: 10_000 });
  await settleLayout(page);
}

async function assertCanvasStateBeforePaneTransition(
  page: Page,
  paneId: string,
  expected: CanvasLayerExpectation,
  label: string,
): Promise<void> {
  const state = await page.evaluate(({ targetPaneId, expectedLayer }) => {
    const canvas = document.querySelector('[data-testid="canvas-view"], [data-testid="plan-view-shell"], [data-testid="canvas-empty"]') as HTMLElement | null;
    if (!canvas) return { kind: "missing-canvas" as const };
    const visible = (el: Element | null): el is HTMLElement => {
      if (!(el instanceof HTMLElement)) return false;
      const rect = el.getBoundingClientRect();
      const style = window.getComputedStyle(el);
      return style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
    };
    const visibleControlsOutsideCanvas = Array.from(document.querySelectorAll(
      '[data-testid="pane-session-chat"], [data-testid="terminal-pane"], [data-testid="chat-composer"], [data-testid="chat-reconnect"]',
    )).filter((el) => visible(el) && !el.closest('[inert], [aria-hidden="true"]') && !canvas.contains(el));
    if (expectedLayer === "inline") {
      return {
        kind: "inline" as const,
        paneControlCount: visibleControlsOutsideCanvas.length,
      };
    }
    const paneRoot = document.querySelector(`[data-pane-id="${targetPaneId}"]`) as HTMLElement | null;
    if (!visible(paneRoot)) {
      return {
        kind: "overlay" as const,
        paneRootVisible: false,
      };
    }
    const rect = paneRoot.getBoundingClientRect();
    const x = Math.min(Math.max(rect.left + rect.width / 2, 0), document.documentElement.clientWidth - 1);
    const y = Math.min(Math.max(rect.top + rect.height / 2, 0), document.documentElement.clientHeight - 1);
    const hit = document.elementFromPoint(x, y);
    return {
      kind: "overlay" as const,
      paneRootVisible: true,
      hitInsidePaneRoot: hit instanceof Node && (hit === paneRoot || paneRoot.contains(hit)),
      hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
    };
  }, { targetPaneId: paneId, expectedLayer: expected });

  expect(state.kind, `${label} valid canvas state before pane transition ${JSON.stringify(state)}`).not.toBe("missing-canvas");
  if (state.kind === "inline") {
    expect(state.paneControlCount, `${label} inline canvas has no outside-canvas chat/terminal controls`).toBe(0);
    return;
  }
  expect(state.paneRootVisible, `${label} overlay target pane root visible ${JSON.stringify(state)}`).toBe(true);
  expect(state.hitInsidePaneRoot, `${label} overlay canvas blocks click-through to pane root ${JSON.stringify(state)}`).toBe(false);
}

async function assertWorkspaceLayerTopmost(page: Page, paneId: string, label: string): Promise<void> {
  await expect
    .poll(
      () => page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`).evaluate((chat) => {
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

  const result = await page.locator(`[data-testid="pane-session-chat"][data-pane-id="${paneId}"]`).evaluate((chat) => {
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

async function auditKnownSurfaces(page: Page, label: string): Promise<void> {
  for (const selector of OPTIONAL_SURFACES) {
    const loc = page.locator(selector).first();
    if (!(await loc.isVisible().catch(() => false))) continue;
    const box = await loc.boundingBox();
    if (!box) continue;
    expect(box.x, `${label} ${selector} left`).toBeGreaterThanOrEqual(-1);
    expect(box.x + box.width, `${label} ${selector} right`).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
    expect(box.y, `${label} ${selector} top`).toBeGreaterThanOrEqual(-1);
    expect(box.y + box.height, `${label} ${selector} bottom`).toBeLessThanOrEqual(page.viewportSize()!.height + 1);
  }
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
    expect(collectorSource).toContain('getAttribute("aria-hidden") === "true"');
    expect(collectorSource).not.toContain("pointerEvents");
    expect(collectorSource).not.toContain("opacity");
    const transitionSource = openSidebarAndSelectTargetPane.toString();
    expect(transitionSource).toContain("api.focusWorkspace(target.workspaceId)");
    expect(transitionSource).toContain('[data-testid="workspace-group"][data-workspace-id="${target.workspaceId}"]');
    expect(transitionSource).toContain('[data-testid="tab-item"][data-tab-id="${target.tabId}"]');
    expect(transitionSource).not.toContain("workspace.click(");
    const canvasStateSource = assertCanvasStateBeforePaneTransition.toString();
    expect(canvasStateSource).toContain('!el.closest(\'[inert], [aria-hidden="true"]\') && !canvas.contains(el)');

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
  } finally {
    await page.context().close();
  }
});

for (const vp of VIEWPORTS) {
  test(`@golden mobile web primary surfaces stay inside the viewport ${vp.name}`, async ({ webBrowser, api, paneCleanup }, testInfo) => {
    const healthJson = await assertHealthMatchesHead();
    await testInfo.attach("health.json", { body: healthJson, contentType: "application/json" });

    const target = await prepareTargetPane(api, paneCleanup);
    const page = await newWebPage(webBrowser, vp);
    try {
      await openApp(page);
      await settleLayout(page);
      await openSidebarAndSelectTargetPane(api, page, target, `${vp.name} initial target`);
      await expect(
        page.locator(`[data-testid="pane-session-chat"][data-pane-id="${target.paneId}"]`),
        `${vp.name} target pane chat visible before canvas`,
      ).toBeVisible({ timeout: 10_000 });
      await openWorkspaceCanvasOverlay(page, `${vp.name} before pane transition`);
      await assertCanvasStateBeforePaneTransition(page, target.paneId, "overlay", `${vp.name} before pane transition`);
      await openSidebarAndSelectTargetPane(api, page, target, `${vp.name} canvas-to-workspace`);
      await assertWorkspaceLayerTopmost(page, target.paneId, `${vp.name} after target row click`);
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
      await auditViewport(page, `${vp.name} peer menu`);
      await page.keyboard.press("Escape").catch(() => {});

      await page.locator('[data-testid="titlebar-settings"]').click();
      await page.locator('[data-testid="settings-menu"]').waitFor({ state: "visible", timeout: 5_000 });
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} settings menu`);
      await auditViewport(page, `${vp.name} settings menu`);
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
        await settleLayout(page);
        await auditKnownSurfaces(page, `${vp.name} new session picker`);
        await auditViewport(page, `${vp.name} new session picker`);
        const workspaceSelect = page.locator('[data-testid="picker-workspace"]').first();
        if (await workspaceSelect.isVisible().catch(() => false)) {
          await workspaceSelect.click();
          await page.locator('[data-testid^="picker-workspace-option-"]').first().waitFor({ state: "visible", timeout: 5_000 });
          await settleLayout(page);
          await auditViewport(page, `${vp.name} picker workspace dropdown`);
          await page.keyboard.press("Escape").catch(() => {});
        }
      }

      await openSidebar(page);
      await page.locator('[data-testid="nav-mesh"]').click();
      await page.locator('[data-testid="mesh-tab-network"]').waitFor({ state: "visible", timeout: 10_000 });
      await settleLayout(page);
      await auditKnownSurfaces(page, `${vp.name} mesh`);
      await auditViewport(page, `${vp.name} mesh`);

      await page.screenshot({ path: testInfo.outputPath(`mobile-overflow-${vp.name}.png`), fullPage: false });
    } finally {
      await page.context().close();
    }
  });
}
