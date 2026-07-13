/**
 * WHAT: mobile overflow covers broad web surfaces at phone widths.
 * WHY: 320px regressions have repeatedly come from fixed-width panels, menus,
 * and dialogs rather than from the terminal/code scroll surfaces themselves.
 * ORACLE: visible surfaces and controls are fully inside the viewport and
 * reachable at their center; document scroll width is only a secondary signal.
 */
import { execFileSync } from "node:child_process";
import { expect, test, type Browser, type Page, API_BASE, readDevToken } from "../../helpers/fixtures";

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
  await page.locator(".ct-sidebar-drawer.is-open").waitFor({ state: "attached", timeout: 5_000 });
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

async function collectViewportOffenders(page: Page) {
  return page.locator(INTERACTIVE).evaluateAll((nodes) => {
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
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
          el.closest(".xterm-accessibility, .xterm-helper-textarea") !== null;
        if (hidden) return null;
        const left = rect.left;
        const right = rect.right;
        const top = rect.top;
        const bottom = rect.bottom;
        const centerX = Math.min(Math.max(left + rect.width / 2, 0), viewportWidth - 1);
        const centerY = Math.min(Math.max(top + rect.height / 2, 0), viewportHeight - 1);
        const hit = document.elementFromPoint(centerX, centerY);
        const reachable = hit === el || (hit instanceof Node && el.contains(hit));
        const inBounds =
          left >= -1 &&
          right <= viewportWidth + 1 &&
          top >= -1 &&
          bottom <= viewportHeight + 1;
        if (inBounds && reachable) return null;
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
          hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
          hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
        };
      })
      .filter(Boolean);
  });
}

async function auditViewport(page: Page, label: string): Promise<void> {
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

  const offenders = await collectViewportOffenders(page);
  expect(offenders, `${label} offscreen or unreachable controls`).toEqual([]);
}

async function assertPeerPickerTopContained(page: Page, label: string): Promise<void> {
  const box = await page.locator('[data-testid="peer-picker-button"]').boundingBox();
  expect(box, `${label} peer picker button box`).not.toBeNull();
  expect(box!.y, `${label} peer picker button top`).toBeGreaterThanOrEqual(-1);
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
  const drawer = page.locator(".ct-sidebar-drawer").first();
  if (!expectedOpen && (await drawer.count()) === 0) {
    await expect(page.locator('[data-testid="web-sidebar-open"]'), `${label} closed drawer trigger`).toBeVisible();
    return;
  }
  await drawer.waitFor({ state: "attached", timeout: 5_000 });
  const result = await drawer.evaluate((el, open) => {
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
  }, expectedOpen);
  expect(result.open, `${label} drawer open state`).toBe(expectedOpen);
  if (expectedOpen) {
    expect(result.ariaHidden, `${label} open drawer aria-hidden`).toBe("false");
    expect(result.inert, `${label} open drawer inert`).toBe(false);
    expect(result.pointerEvents, `${label} open drawer pointer events`).toBe("auto");
    expect(result.left, `${label} open drawer left`).toBeGreaterThanOrEqual(-1);
    expect(result.visibleWidth, `${label} open drawer visible width`).toBeLessThanOrEqual(result.maxAllowed);
    expect(result.right, `${label} open drawer right`).toBeLessThanOrEqual(result.viewportWidth + 1);
  } else {
    expect(result.ariaHidden, `${label} closed drawer aria-hidden`).toBe("true");
    expect(result.inert, `${label} closed drawer inert`).toBe(true);
    expect(result.pointerEvents, `${label} closed drawer pointer events`).toBe("none");
    expect(result.right, `${label} closed drawer offscreen`).toBeLessThanOrEqual(1);
    const closedInteractive = drawer.locator(INTERACTIVE).first();
    if ((await closedInteractive.count()) > 0) {
      await closedInteractive.focus().catch(() => {});
      const focusedInside = await drawer.evaluate((el) => el.contains(document.activeElement));
      expect(focusedInside, `${label} closed drawer focus trap`).toBe(false);
    }
    const hitInside = await drawer.evaluate((el) => {
      const rect = el.getBoundingClientRect();
      const x = Math.min(Math.max(rect.left + Math.max(rect.width, 1) / 2, 0), document.documentElement.clientWidth - 1);
      const y = Math.min(Math.max(rect.top + Math.max(rect.height, 1) / 2, 0), document.documentElement.clientHeight - 1);
      const hit = document.elementFromPoint(x, y);
      return !!hit && el.contains(hit);
    });
    expect(hitInside, `${label} closed drawer hit target`).toBe(false);
    const backdrop = page.locator(".ct-sidebar-backdrop").first();
    if ((await backdrop.count()) > 0) {
      const backdropState = await backdrop.evaluate((el) => ({
        ariaHidden: el.getAttribute("aria-hidden"),
        pointerEvents: window.getComputedStyle(el).pointerEvents,
      }));
      expect(backdropState.ariaHidden, `${label} closed backdrop aria-hidden`).toBe("true");
      expect(backdropState.pointerEvents, `${label} closed backdrop pointer events`).toBe("none");
    }
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
      </style>
      <div id="root">
        <div id="scroll-owner">
          <button id="clipped-descendant" data-testid="clipped-descendant">clipped</button>
        </div>
        <button id="offscreen-unowned" data-testid="offscreen-unowned">offscreen</button>
        <button id="partial-unowned" data-testid="partial-unowned">partial</button>
      </div>
    `);

    const offenders = await collectViewportOffenders(page);
    const ids = offenders.map((item) => item?.testid).sort();
    expect(ids).not.toContain("clipped-descendant");
    expect(ids).toContain("offscreen-unowned");
    expect(ids).toContain("partial-unowned");
  } finally {
    await page.context().close();
  }
});

for (const vp of VIEWPORTS) {
  test(`@golden mobile web primary surfaces stay inside the viewport ${vp.name}`, async ({ webBrowser }, testInfo) => {
    const healthJson = await assertHealthMatchesHead();
    await testInfo.attach("health.json", { body: healthJson, contentType: "application/json" });

    const page = await newWebPage(webBrowser, vp);
    try {
      await openApp(page);
      await settleLayout(page);
      await assertPeerPickerTopContained(page, `${vp.name} initial`);
      await auditViewport(page, `${vp.name} initial`);
      await auditSidebarDrawer(page, `${vp.name} closed sidebar`, false);

      await openSidebar(page);
      await auditSidebarDrawer(page, `${vp.name} open sidebar`, true);
      await auditViewport(page, `${vp.name} open sidebar`);
      await closeSidebar(page);
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
      await auditViewport(page, `${vp.name} sidebar tree`);
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
