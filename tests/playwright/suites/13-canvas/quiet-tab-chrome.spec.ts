/**
 * WHAT: quiet tab chrome covers its Playwright acceptance contract.
 * WHY: This spec protects the live CodeTerm workflow from regressions in 13-canvas.
 * ORACLE: The test asserts backend state and/or stable UI hooks instead of relying on smoke-only visibility.
 */
import { expect, test } from "../../helpers/fixtures";

test("@golden quiet tab chrome exposes calm activity hooks, not task bubbles", async ({ ui }) => {
  const tab = ui.page.locator('[data-testid="tab-item"]').first();
  await expect(tab).toBeVisible({ timeout: 30_000 });
  await expect(tab).toHaveAttribute("data-tab-no-task-bubble", "true");
  await expect(ui.page.locator('[data-testid="tab-task-bubble"]')).toHaveCount(0);
  await expect(ui.page.locator('[data-testid="tab-focus-bubble"]')).toHaveCount(0);
  await expect(ui.page.locator('[data-testid="tab-plan-focus-chip"]')).toHaveCount(0);

  const activity = ui.page.locator('[data-testid="tab-activity"]');
  const count = await activity.count();
  if (count > 0) {
    await expect(activity.first()).toHaveAttribute("data-tab-activity-quiet", "true");
    await expect(activity.first()).not.toHaveClass(/ct-presence-live/);
    await expect(activity.first()).not.toHaveClass(/animate-pulse/);
  }
});
