import { expect, test } from "../../helpers/fixtures";

test("@golden @workspace-plan-entry sidebar Plan entry opens workspace canvas", async ({ ui }) => {
  const { page } = ui;

  const workspace = page.locator('[data-testid="workspace-item"][data-workspace-name="codeterm"]').first();
  await expect(workspace).toBeVisible({ timeout: 30_000 });
  const expand = workspace.locator('button[title="Expand"]');
  if (await expand.isVisible().catch(() => false)) {
    await expand.click();
  }

  const entry = page.locator('[data-testid="workspace-plan-entry"]').first();
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await entry.click();

  await expect(page.locator('[data-testid="canvas-view"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(
    page.locator('[data-testid="canvas-plan-node"], [data-testid="canvas-empty"]').first(),
  ).toBeVisible({ timeout: 30_000 });
});
