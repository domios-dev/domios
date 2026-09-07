/**
 * WHAT: plan header mobile covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import path from "node:path";
import { expect, test } from "../../helpers/fixtures";
import {
  assertPlanSurfaceNotEmpty,
  openInlineWorkspaceCanvas,
  seedPerfFixtureAndOpenInlineCanvas,
  CANVAS_ART_DIR,
  waitForCanvasSettled,
} from "../../helpers/canvas-inspect";
import type { ApiClient } from "../../helpers/api-client";
import type { Locator, Page } from "@playwright/test";

const PHONE = { width: 390, height: 844 };
const DESKTOP = { width: 1280, height: 800 };
// Keep the outer deadline bounded so helper diagnostics remain visible instead of collapsing to an opaque timeout.
const TEST_TIMEOUT = 120_000;

function visibleHeader(page: Page): Locator {
  return page.locator('[data-testid="plan-context-header"]:visible').first();
}

async function shot(page: Page, name: string): Promise<string> {
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}

async function applyViewportAndReload(
  page: Page,
  workspaceId: string,
  vp: { width: number; height: number },
  mode: "board" | "list" = "board",
): Promise<void> {
  await page.setViewportSize(vp);
  const shell = page.locator('[data-testid="plan-view-shell"]').first();
  if (!(await shell.isVisible().catch(() => false))) {
    await openInlineWorkspaceCanvas(page, workspaceId);
  }
  if (vp.width < 640 && !(await page.locator('[data-testid="web-sidebar-open"]').isVisible().catch(() => false))) {
    // App.tsx decides `sidebarOpen` ONCE, from window.innerWidth at mount, and the page is opened at 1280px — so resizing to a phone leaves the overlay open.
    await page.locator(".ct-sidebar-backdrop").click();
    await page.locator('[data-testid="web-sidebar-open"]').waitFor({ state: "visible", timeout: 5_000 });
  }
  // which mounts no plan-context header. Force a task surface without reloading.
  if (mode !== "canvas") {
    await shell.locator('[data-testid="plan-view-mode-switch"]').getByRole("tab", { name: new RegExp(mode, "i") }).click();
  }
  await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
  await page.waitForSelector('[data-testid="plan-context-header"]:visible', { state: "visible", timeout: 30_000 });
}

test(
  "@golden @plan-header-mobile phone: one-bar compact header, tasks above the fold",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE);

      const header = visibleHeader(page);
      await expect(header.locator('[data-testid="plan-context-compact-bar"]')).toBeVisible();
      await expect(header).toBeVisible();
      const box = await header.boundingBox();
      expect(box, "header has a box").toBeTruthy();
      expect(box!.height, "collapsed compact header <= 56px").toBeLessThanOrEqual(56);

      // Collapsed by default: expanded panel not mounted, orientation bits present.
      await expect(header.locator('[data-testid="plan-context-expanded"]')).toHaveCount(0);
      await expect(header.locator('[data-testid="plan-context-compact-title"]')).toBeVisible();
      await expect(header.locator('[data-testid="plan-context-compact-progress"]')).toBeVisible();

      // Tasks visible above the fold: first board column top edge in the upper half.
      // The seeded board always has columns, so this is unconditional.
      const col = page.locator('[data-testid="plan-board-column"]:visible').first();
      await expect(col).toBeVisible();
      const colBox = await col.boundingBox();
      expect(colBox, "column has a box").toBeTruthy();
      expect(colBox!.y, "first column starts in the upper half of the viewport").toBeLessThan(
        PHONE.height / 2,
      );
      await shot(page, "plan-header-mobile-collapsed");
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile phone: tap expands inline and choice persists",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE);

      const bar = visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]');
      await expect(bar).toHaveAttribute("aria-expanded", "false");
      await bar.click();
      await expect(bar).toHaveAttribute("aria-expanded", "true");

      const expanded = visibleHeader(page).locator('[data-testid="plan-context-expanded"]');
      await expect(expanded).toBeVisible();
      await expect(expanded.locator('[data-testid="plan-board-plan-goal"]')).toBeVisible();
      await expect(expanded.locator('[data-testid="plan-context-header-chips"]')).toBeVisible();
      await shot(page, "plan-header-mobile-expanded");

      // Persist across reload (UI pref, per width class).
      await page.reload({ waitUntil: "domcontentloaded" });
      await page.waitForSelector('[data-testid="plan-context-compact-bar"]:visible', { state: "visible", timeout: 30_000 });
      await expect(visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]')).toHaveAttribute(
        "aria-expanded",
        "true",
      );

      // Collapse again and confirm inline collapse.
      await visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]').click();
      await expect(visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]')).toHaveAttribute(
        "aria-expanded",
        "false",
      );
      await expect(visibleHeader(page).locator('[data-testid="plan-context-expanded"]')).toHaveCount(0);
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile desktop: no compact bar, full context intact",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, DESKTOP);

      // No compact bar anywhere (neither mounted view renders one above the breakpoint).
      await expect(page.locator('[data-testid="plan-context-compact-bar"]')).toHaveCount(0);
      const header = visibleHeader(page);
      await expect(header).toBeVisible();
      await expect(header.locator('[data-testid="plan-context-header-chips"]')).toBeVisible();
      await expect(header.locator('[data-testid="plan-board-plan-title"]')).toBeVisible();

      // rows keep their compact (non-touch) classes above the breakpoint.
      const navCls = (await page.locator('[data-testid="nav-extensions"]').first().getAttribute("class")) ?? "";
      expect(navCls, "nav row has no touch min-height on desktop").not.toContain("min-h-[44px]");
      const nsCount = await page.locator('[data-testid="sidebar-new-session"]').count();
      if (nsCount > 0) {
        const nsCls = (await page.locator('[data-testid="sidebar-new-session"]').first().getAttribute("class")) ?? "";
        expect(nsCls, "new-session has no touch min-height on desktop").not.toContain("min-h-[44px]");
      }
      await shot(page, "plan-header-desktop-unchanged");
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile plan header hides empty and failed skills states",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      // A failed skills endpoint must not add an error surface to the header.
      await page.route("**/api/plans/*/skills", (route) =>
        route.fulfill({ status: 500, contentType: "application/json", body: '{"error":"boom"}' }),
      );
      await applyViewportAndReload(page, canvas.workspaceId, PHONE);

      const header = visibleHeader(page);
      await header.locator('[data-testid="plan-context-compact-bar"]').click();
      await expect(header.locator('[data-testid="plan-context-expanded"]')).toBeVisible();

      await expect(header.locator('[data-testid="plan-skills-section"]')).toHaveCount(0);
      await expect(header.locator('[data-testid="plan-skills-error"]')).toHaveCount(0);
      await shot(page, "plan-header-skills-absent");
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile a11y: bar is a focusable button, chevron labeled",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE);

      const bar = visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]');
      expect(await bar.evaluate((el) => el.tagName)).toBe("BUTTON");
      await bar.focus();
      const outline = await bar.evaluate((el) => {
        const s = getComputedStyle(el);
        return { outlineStyle: s.outlineStyle, outlineWidth: s.outlineWidth };
      });
      expect(outline.outlineStyle, "focus outline visible").not.toBe("none");
      const chevronLabel = await bar
        .locator(".plan-context-compact-chevron")
        .getAttribute("aria-label");
      expect(chevronLabel ?? "", "chevron has an aria-label").toMatch(/expand|collapse/i);
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile list: filters + in-flight strip stay below the compact bar",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE, "list");

      const header = visibleHeader(page);
      await expect(header.locator('[data-testid="plan-context-compact-bar"]')).toBeVisible();
      const filter = page.locator('[data-testid="plan-list-filter"]:visible').first();
      await expect(filter, "status filters remain visible on narrow").toBeVisible();

      // Filters sit BELOW the compact bar and never fold.
      await expect(header, "compact header present at phone width").toBeVisible();
      const headerBox = await header.boundingBox();
      const filterBox = await filter.boundingBox();
      expect(headerBox && filterBox, "both have boxes").toBeTruthy();
      expect(filterBox!.y, "filters are below the compact header").toBeGreaterThanOrEqual(
        headerBox!.y + headerBox!.height - 1,
      );
      await shot(page, "plan-header-mobile-list-interplay");
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile coarse hit targets: compact bar + sidebar rows >= 44px",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE, "board");

      // Named post-condition: if the board is gone by here, say so in seconds
      // instead of letting an unbounded boundingBox() burn the whole test budget.
      await expect(
        page.locator('[data-testid="plan-view-shell"]'),
        "plan view shell still mounted after the viewport switch",
      ).toHaveAttribute("data-plan-view-mode", "board");
      await assertPlanSurfaceNotEmpty(page);

      // Compact plan bar is a >=44px coarse target.
      const bar = visibleHeader(page).locator('[data-testid="plan-context-compact-bar"]');
      await expect(bar, "compact bar present at phone width").toBeVisible({ timeout: 15_000 });
      const barBox = await bar.boundingBox();
      expect(barBox, "compact bar has a box").toBeTruthy();
      expect(barBox!.height, "compact bar >= 44px").toBeGreaterThanOrEqual(44);

      // Open the sidebar overlay (phones start collapsed).
      const opener = page.locator('[data-testid="web-sidebar-open"]');
      if (await opener.count()) {
        await opener.first().click();
        await waitForCanvasSettled(page, 2_500);
      }

      // `workspace-plan-entry` was deleted from product source (a plan's sidebar home is now its Team card), so it is not in this sweep.
      const ROW_TESTIDS = [
        "tab-item",
        "workspace-item",
        "nav-extensions",
        "nav-memory",
        "nav-mesh",
        "sidebar-new-session",
        "recently-closed-item",
        "recently-closed-group-toggle",
      ];
      // Use one atomic DOM read per id so a detaching row fails instead of hanging the sweep.
      let asserted = 0;
      for (const id of ROW_TESTIDS) {
        const heights = await page
          .locator(`[data-testid="${id}"]:visible`)
          .evaluateAll((els) => els.map((el) => el.getBoundingClientRect().height));
        heights.forEach((height, i) => {
          expect(height, `${id}[${i}] hit height >= 44px`).toBeGreaterThanOrEqual(44);
          asserted += 1;
        });
      }
      // a false green if selectors silently match nothing.
      expect(asserted, "at least the nav + new-session rows were measured").toBeGreaterThanOrEqual(4);
      await shot(page, "plan-header-mobile-hit-targets");
    } finally {
      await canvas.close();
    }
  },
);

test(
  "@golden @plan-header-mobile companion bubble does not overlap compact header or composer",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient);
    try {
      const { page } = canvas;
      await assertPlanSurfaceNotEmpty(page);
      await applyViewportAndReload(page, canvas.workspaceId, PHONE);

      // The companion bubble cannot mount with synthetic owner ids, so its geometry is not observable in these fixtures.
      await expect(page.locator('[data-testid="plan-companion-bubble"]')).toHaveCount(0);
      await shot(page, "plan-header-mobile-companion");
    } finally {
      await canvas.close();
    }
  },
);
