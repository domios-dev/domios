/**
 * 13-canvas - tag legend facet filter: click legend value -> matching tasks
 * highlight, non-matching dim. Tile tag payload covered by TaskCard unit tests.
 *
 * Skips when the dev workspace has no color-lane tag facets on active plans.
 *
 *   npx playwright test --config=tests/playwright/playwright.config.ts \
 *     --project=golden --grep @canvas-tag-filter
 */
import { expect, test } from "../../helpers/fixtures";
import { isEmpty, openCanvas } from "../../helpers/canvas-inspect";

test("@golden @canvas-tag-filter legend click highlights matching tasks and dims others", async ({ api, ui }) => {
  const { page } = ui;

  const workspaces = await api.workspaces();
  const repoWorkspace = workspaces.find((w) => w.cwd === "/Users/rollacode/Developer/codeterm");
  if (repoWorkspace) await api.focusWorkspace(repoWorkspace.id);
  await page.reload({ waitUntil: "domcontentloaded" });

  await openCanvas(page);
  if (await isEmpty(page)) {
    test.skip(true, "no active plans in the dev workspace");
    return;
  }

  const legend = page.locator('[data-testid="canvas-tag-legend"]');
  if ((await legend.count()) === 0) {
    test.skip(true, "no color-lane tag facets on active plans");
    return;
  }

  const value = legend.locator('[data-testid="canvas-tag-legend-value"]').first();
  await expect(value).toBeVisible();
  const tag = await value.getAttribute("data-tag");
  if (!tag) {
    test.skip(true, "legend value missing data-tag");
    return;
  }

  const cards = page.locator('[data-testid="canvas-task-card"]');
  const cardCount = await cards.count();
  if (cardCount === 0) {
    test.skip(true, "no task cards on board");
    return;
  }

  await value.click();
  await expect(value).toHaveAttribute("data-selected", "true");

  const highlighted = page.locator('[data-testid="canvas-task-card"][data-tag-filter="highlight"]');
  const dimmed = page.locator('[data-testid="canvas-task-card"][data-tag-filter="dimmed"]');
  await expect(highlighted.first()).toBeVisible({ timeout: 5000 });

  const highlightCount = await highlighted.count();
  const dimCount = await dimmed.count();
  expect(highlightCount + dimCount, "filtered tasks partition the board").toBe(cardCount);
  expect(highlightCount, "at least one task matches the facet filter").toBeGreaterThan(0);

  const tileTags = page.locator('[data-testid="canvas-task-tile-tags"]');
  if ((await tileTags.count()) > 0) {
    await expect(tileTags.first()).toBeVisible();
  }

  await value.click();
  await expect(value).toHaveAttribute("data-selected", "false");
  await expect(page.locator('[data-testid="canvas-task-card"][data-tag-filter="dimmed"]')).toHaveCount(0);
});
