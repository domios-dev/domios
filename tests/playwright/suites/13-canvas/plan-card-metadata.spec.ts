/**
 * WHAT: plan card metadata covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "../../helpers/fixtures";
import {
  CANVAS_ART_DIR,
  CARD_METADATA_PROFILE,
  CARD_METADATA_PROFILE_KEY,
  isEmpty,
  seedPerfFixtureAndOpenInlineCanvas,
} from "../../helpers/canvas-inspect";
import type { ApiClient } from "../../helpers/api-client";
import { CARD_METADATA_TASK_IDS } from "../../../../src/components/Canvas/cardMetadataFixture";

const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 844 };
const TEST_TIMEOUT = 120_000;

async function shot(page: Page, name: string): Promise<string> {
  fs.mkdirSync(CANVAS_ART_DIR, { recursive: true });
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}

async function shotCard(page: Page, taskId: string, name: string): Promise<void> {
  const card = page.locator(`[data-testid="plan-board-task-card"][data-task-id="${taskId}"]:visible`).first();
  await expect(card).toBeVisible({ timeout: 15_000 });
  const file = path.join(CANVAS_ART_DIR, `${name}.png`);
  await card.screenshot({ path: file });
}

async function activateCardMetadataFixture(page: Page): Promise<void> {
  await page.evaluate(
    ([profileKey, profile]) => {
      window.localStorage.setItem(profileKey, profile);
    },
    [CARD_METADATA_PROFILE_KEY, CARD_METADATA_PROFILE],
  );
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector('[data-testid="plan-view-shell"]', { state: "visible", timeout: 30_000 });
}

async function switchPlanMode(page: Page, mode: "board" | "list"): Promise<void> {
  const shell = page.locator('[data-testid="plan-view-shell"]').first();
  await shell.locator('[data-testid="plan-view-mode-switch"]').getByRole("tab", { name: new RegExp(mode, "i") }).click();
  await expect(shell).toHaveAttribute("data-plan-view-mode", mode);
}

async function expectWorkingOnChipValue(
  card: import("@playwright/test").Locator,
  kind: string,
  pattern: RegExp | string,
): Promise<void> {
  const chipValue = card.locator(`[data-testid="working-on-chip-${kind}"] .working-on-chip-value`);
  if ((await chipValue.count()) === 0) {
    const trigger = card.locator('[data-testid="working-on-overflow-trigger"]');
    await expect(trigger).toBeVisible();
    await trigger.click();
    await expect(card.locator('[data-testid="working-on-overflow-popover"]')).toBeVisible();
  }
  await expect(card.locator(`[data-testid="working-on-chip-${kind}"] .working-on-chip-value`)).toContainText(pattern);
}

async function openBoardWithFixture(api: ApiClient, webBrowser: import("@playwright/test").Browser) {
  const canvas = await seedPerfFixtureAndOpenInlineCanvas(webBrowser, api as unknown as ApiClient, 1);
  const { page } = canvas;
  if (await isEmpty(page)) {
    return { canvas, ready: false as const };
  }
  await activateCardMetadataFixture(page);
  await switchPlanMode(page, "board");
  await expect(page.locator('[data-testid="plan-board-view"]:visible')).toBeVisible({ timeout: 15_000 });
  return { canvas, ready: true as const, page };
}

async function expectCardsContained(page: Page): Promise<void> {
  const violations = await page.locator('[data-testid="plan-board-column"]').evaluateAll((columns) =>
    columns.flatMap((column) => {
      const cards = Array.from(column.querySelectorAll<HTMLElement>('[data-testid="plan-board-task-card"]'));
      const issues: string[] = [];
      cards.forEach((card, index) => {
        const cardRect = card.getBoundingClientRect();
        for (const meta of card.querySelectorAll<HTMLElement>('[data-testid^="plan-board-task-"]')) {
          const rect = meta.getBoundingClientRect();
          if (rect.top < cardRect.top - 0.5 || rect.bottom > cardRect.bottom + 0.5) {
            issues.push(`${card.dataset.taskId}: metadata escapes card bounds`);
          }
        }
        const next = cards[index + 1];
        if (next && next.getBoundingClientRect().top < cardRect.bottom - 0.5) {
          issues.push(`${card.dataset.taskId}: overlaps ${next.dataset.taskId}`);
        }
      });
      return issues;
    }),
  );
  expect(violations).toEqual([]);
}

test(
  "@golden @plan-card-metadata desktop board and list metadata album with per-status cards",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const opened = await openBoardWithFixture(api as unknown as ApiClient, webBrowser);
    try {
      if (!opened.ready) {
        test.skip(true, "canvas empty — card-metadata fixture did not inject");
        return;
      }
      const { page } = opened;
      await page.setViewportSize(DESKTOP);
      await expect(page.locator('[data-testid="plan-board-task-card"]').first()).toBeVisible();

      await shot(page, "plan-card-metadata-board-desktop");
      await shotCard(page, CARD_METADATA_TASK_IDS.pending, "plan-card-metadata-pending");
      await shotCard(page, CARD_METADATA_TASK_IDS.live, "plan-card-metadata-live");
      await shotCard(page, CARD_METADATA_TASK_IDS.done, "plan-card-metadata-done");
      await shotCard(page, CARD_METADATA_TASK_IDS.blocked, "plan-card-metadata-blocked");

      await switchPlanMode(page, "list");
      await expect(page.locator('[data-testid="plan-list-view"]:visible')).toBeVisible();
      await expect(page.locator('[data-testid="plan-list-task"]').first()).toBeVisible();
      await shot(page, "plan-card-metadata-list-desktop");
    } finally {
      await opened.canvas.close();
    }
  },
);

test(
  "@golden @plan-card-metadata 390px viewport board and list metadata album",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const opened = await openBoardWithFixture(api as unknown as ApiClient, webBrowser);
    try {
      if (!opened.ready) {
        test.skip(true, "canvas empty — card-metadata fixture did not inject");
        return;
      }
      const { page } = opened;
      await page.setViewportSize(PHONE);
      await expect(page.locator('[data-testid="plan-board-task-card"]').first()).toBeVisible();
      if (!(await page.locator('[data-testid="web-sidebar-open"]').isVisible().catch(() => false))) {
        await page.mouse.click(PHONE.width - 8, Math.floor(PHONE.height / 2));
      }

      await shot(page, "plan-card-metadata-board-390");
      await switchPlanMode(page, "list");
      await shot(page, "plan-card-metadata-list-390");
    } finally {
      await opened.canvas.close();
    }
  },
);

test(
  "@golden @plan-card-metadata lane warning chips: branch truncation, no-tab, tab-gone",
  async ({ api, webBrowser }) => {
    test.setTimeout(TEST_TIMEOUT);
    const opened = await openBoardWithFixture(api as unknown as ApiClient, webBrowser);
    try {
      if (!opened.ready) {
        test.skip(true, "canvas empty — card-metadata fixture did not inject");
        return;
      }
      const { page } = opened;
      await page.setViewportSize(DESKTOP);

      const longBranchCard = page.locator(
        `[data-testid="plan-board-task-card"][data-task-id="${CARD_METADATA_TASK_IDS.longBranch}"]`,
      );
      await expectWorkingOnChipValue(longBranchCard, "branch", "…");
      await shotCard(page, CARD_METADATA_TASK_IDS.longBranch, "plan-card-metadata-long-branch");

      const noTabCard = page.locator(
        `[data-testid="plan-board-task-card"][data-task-id="${CARD_METADATA_TASK_IDS.noTab}"]`,
      );
      await expectWorkingOnChipValue(noTabCard, "warn", /no tab recorded/i);
      await shotCard(page, CARD_METADATA_TASK_IDS.noTab, "plan-card-metadata-no-tab");

      const tabGoneCard = page.locator(
        `[data-testid="plan-board-task-card"][data-task-id="${CARD_METADATA_TASK_IDS.tabGone}"]`,
      );
      await expectWorkingOnChipValue(tabGoneCard, "warn", /tab gone/i);
      await shotCard(page, CARD_METADATA_TASK_IDS.tabGone, "plan-card-metadata-tab-gone");

      const branchMissingCard = page.locator(
        `[data-testid="plan-board-task-card"][data-task-id="${CARD_METADATA_TASK_IDS.branchMissing}"]`,
      );
      await expectWorkingOnChipValue(branchMissingCard, "warn", /branch missing/i);
      await shotCard(page, CARD_METADATA_TASK_IDS.branchMissing, "plan-card-metadata-branch-missing");

      const doneCard = page.locator(
        `[data-testid="plan-board-task-card"][data-task-id="${CARD_METADATA_TASK_IDS.done}"]`,
      );
      await expect(doneCard.locator('[data-testid="working-on-row"]')).toHaveCount(0);
      await expect(doneCard).not.toContainText(/tab gone/i);
      await expect(doneCard.locator('[data-testid="plan-board-task-historical-executor"]')).toBeVisible();
    } finally {
      await opened.canvas.close();
    }
  },
);

for (const viewport of [DESKTOP, PHONE]) {
  test(
    `@golden @plan-card-metadata stacked cards stay contained at ${viewport.width}px`,
    async ({ api, webBrowser }) => {
      const opened = await openBoardWithFixture(api as unknown as ApiClient, webBrowser);
      try {
        if (!opened.ready) {
          test.skip(true, "canvas empty — card-metadata fixture did not inject");
          return;
        }
        const { page } = opened;
        await page.setViewportSize(viewport);
        await expectCardsContained(page);
        const branchValue = page
          .locator('[data-testid="plan-board-task-lane"] .plan-board-task-meta-value')
          .first();
        await expect(branchValue).toHaveCSS("text-overflow", "ellipsis");
        await expect(branchValue).toHaveCSS("overflow-x", "hidden");
        await expect(branchValue).toHaveCSS("white-space", "nowrap");
      } finally {
        await opened.canvas.close();
      }
    },
  );
}
