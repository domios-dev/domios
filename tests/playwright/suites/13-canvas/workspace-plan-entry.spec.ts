import { expect, test } from "../../helpers/fixtures";

test("@golden @workspace-plan-entry sidebar Plan entry opens workspace canvas", async ({ ui }) => {
  const { page } = ui;

  const workspace = page.locator('[data-testid="workspace-item"][data-workspace-name="codeterm"]').first();
  await expect(workspace).toBeVisible({ timeout: 30_000 });
  const expand = workspace.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) {
    await expand.click();
  }

  const general = page.locator('[data-testid="workspace-item"][data-workspace-name="General"]').first();
  await expect(general).toBeVisible({ timeout: 30_000 });
  const generalBox = await general.boundingBox();
  const firstEntryBox = await page.locator('[data-testid="workspace-plan-entry"]').first().boundingBox();
  expect(Boolean(generalBox && firstEntryBox && firstEntryBox.y > generalBox.y + generalBox.height)).toBeTruthy();

  const entry = page.locator('[data-testid="workspace-plan-entry"]').first();
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-workspace-action", "plan");
  await expect(entry).toHaveAttribute("data-pinned", "true");
  await expect(entry).toHaveText(/Plan/);
  await entry.click();

  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-active", "true");
  await expect(page.getByTitle("Close plan canvas")).toHaveCount(0);

  const tab = page.locator('[data-testid="tab-item"]').filter({ hasText: "Curie" }).first();
  await expect(tab).toBeVisible({ timeout: 30_000 });
  await tab.click({ force: true });
  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeHidden({ timeout: 30_000 });
  await expect(entry).toHaveAttribute("data-active", "false");
});
